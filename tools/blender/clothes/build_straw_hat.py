"""Build ChapeuPalha.glb: a wide-brimmed peasant straw hat for chartest (accessories/).

Rigid on the `head` bone, exported like the hairs (vertices relative to the head pivot under a
`head_socket` empty), so the engine attaches it with the head's inverse bind.

The crown is fitted to the head plus the hairs that can be worn under it (short, long loose and
bald): for each angle around the head and each height, the crown's inner wall stands `GAP`
outside the farthest point of head or hair above that height. Buns are left out (they poke
through), so the game must not give the hat to anyone with a bun.

blender --background --python tools/blender/clothes/build_straw_hat.py
"""

import json
import math
import pathlib
import random
import struct

import bmesh
import bpy
from mathutils import Matrix, Vector

ROOT = pathlib.Path(r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE")
CHARTEST_BLEND = ROOT / "assets/models/chartest/chartest.blend"
HAIRS = ROOT / "assets/models/chartest/hairs"
UNDER = ["CabeloCurtoMasculino", "CabeloLongoSolto"]
CHECK_ONLY = ["CoqueSamuraiMasculino", "CoqueSoltoFeminino"]
OUT_DIR = ROOT / "tools/blender/clothes/out/straw_hat"
NAME = "ChapeuPalha"

SEGMENTS = 16          # around the hat
BAND_Z = 1.742         # where the crown meets the brim (above brow 1.718 and ear top 1.7065)
GAP = 0.006            # inner crown wall to the farthest point of the head
WALL = 0.006           # crown thickness
BRIM_OUT = 0.080       # how far the brim reaches out past the crown wall
BRIM_THICK = 0.007
DROOP = 0.024          # how far the brim edge hangs below the band
CROWN_LEVELS = [0.0, 0.025, 0.05, 0.07, 0.085]  # heights of the crown rings above the band (scaled)
SEED = 7


def head_and_hair_points(body_obj, names):
    basis = body_obj.data.shape_keys.key_blocks[0].data
    pts = [basis[i].co.copy() for i in range(len(basis)) if basis[i].co.z > BAND_Z - 0.03]
    hair = {}
    for name in names:
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(HAIRS / f"{name}.glb"))
        new = [o for o in bpy.data.objects if o not in before]
        bpy.context.view_layer.update()
        hp = [o.matrix_world @ v.co for o in new if o.type == "MESH" for v in o.data.vertices]
        hair[name] = [p for p in hp if p.z > BAND_Z - 0.03]
        for o in new:
            bpy.data.objects.remove(o, do_unlink=True)
    return pts, hair


def polar(p, c):
    d = Vector((p.x - c.x, p.y - c.y))
    return math.atan2(d.x, d.y), d.length  # 0 = +Y (face), +pi/2 = +X


def fit_crown(points, c):
    """Inner crown: an ellipse per level, just wide enough for every point above that level.

    Returns the levels, the inner radius at each segment and level, and the inner top height.
    """
    # Hanging strands of long hair are not the skull: only what hugs the head counts.
    points = [p for p in points if polar(p, c)[1] < 0.16]
    top = max(p.z for p in points) + GAP
    span = top - BAND_Z
    levels = [BAND_Z + (span - 0.010) * h / CROWN_LEVELS[-1] for h in CROWN_LEVELS]
    # Per angle, not an ellipse: an ellipse grown to hold an egg-shaped skull ends up much
    # longer than the head front to back.
    radii = [[0.0] * len(levels) for _ in range(SEGMENTS)]
    window = math.pi / SEGMENTS
    for k, z in enumerate(levels):
        # The wall leans in between rings, so a ring must hold everything up from the ring below.
        floor = levels[k - 1] if k else z
        above = [polar(p, c) for p in points if p.z >= floor - 0.004]
        for s in range(SEGMENTS):
            a = 2 * math.pi * s / SEGMENTS
            best = 0.03
            for ang, r in above:
                diff = abs((ang - a + math.pi) % (2 * math.pi) - math.pi)
                if diff <= 2 * window:
                    best = max(best, r * math.cos(diff))
            radii[s][k] = best + GAP
        # A chord between two corners cuts inside them; and keep the outline convex.
        for s in range(SEGMENTS):
            radii[s][k] /= math.cos(window)
        col = [radii[s][k] for s in range(SEGMENTS)]
        for s in range(SEGMENTS):
            radii[s][k] = max(col[s], 0.5 * (col[s - 1] + col[(s + 1) % SEGMENTS]))
    for row in radii:
        for k in range(len(row) - 2, -1, -1):
            row[k] = max(row[k], row[k + 1])
    return levels, radii, top


def build_hat(c, levels, radii, top):
    rnd = random.Random(SEED)
    wobble = [rnd.uniform(-0.012, 0.012) for _ in range(SEGMENTS)]
    sag = [rnd.uniform(-0.006, 0.006) for _ in range(SEGMENTS)]
    bm = bmesh.new()
    uv = bm.loops.layers.uv.new("UVMap")
    rings = []  # each ring: list of (vert, v texture coordinate), one per segment
    vs = []

    def ring(fn, v):
        out = []
        for s in range(SEGMENTS):
            a = 2 * math.pi * s / SEGMENTS
            r, z = fn(s)
            out.append(bm.verts.new((c.x + math.sin(a) * r, c.y + math.cos(a) * r, z)))
        rings.append(out)
        vs.append(v)

    n = len(levels)
    # Texture rows (v, 0 = bottom of the image): straw plait 0.30..1, band 0.18..0.30, brim
    # underside 0..0.18.
    # Inner crown, from the top down.
    for k in range(n - 1, -1, -1):
        ring(lambda s, k=k: (radii[s][k], levels[k]), 0.02 + 0.14 * k / n)
    # Brim underside, from the crown out.
    for t in (0.35, 1.0):
        def under(s, t=t):
            r0 = radii[s][0]
            r = r0 + (BRIM_OUT + wobble[s]) * t
            return r, BAND_Z - BRIM_THICK - (DROOP + sag[s]) * t * t
        ring(under, 0.04 + 0.12 * t)
    # Brim top, from the edge in.
    for t in (1.0, 0.65, 0.3, 0.0):
        def over(s, t=t):
            r0 = radii[s][0] + WALL
            r = r0 + (BRIM_OUT + wobble[s] + 0.002 - WALL) * t
            return r, BAND_Z - (DROOP + sag[s]) * t * t + (0.0 if t else 0.0)
        ring(over, 0.32 + 0.30 * (1.0 - t))
    # Hatband on the crown, then the outer crown upward.
    ring(lambda s: (radii[s][0] + WALL + 0.002, BAND_Z + 0.016), 0.19)
    ring(lambda s: (radii[s][0] + WALL + 0.002, BAND_Z + 0.017), 0.30)
    for k in range(1, n):
        z = levels[k] + WALL if k < n - 1 else top + WALL - 0.004
        ring(lambda s, k=k, z=z: (radii[s][k] + WALL, z), 0.64 + 0.30 * k / (n - 1))

    # Faces between consecutive rings (order goes inside -> outside -> top, normals outward).
    def quad(a, b, va, vb, s):
        s2 = (s + 1) % SEGMENTS
        f = bm.faces.new((a[s], a[s2], b[s2], b[s]))
        u0, u1 = s / SEGMENTS, (s + 1) / SEGMENTS
        for loop, co in zip(f.loops, ((u0, va), (u1, va), (u1, vb), (u0, vb))):
            loop[uv].uv = co

    for i in range(len(rings) - 1):
        for s in range(SEGMENTS):
            quad(rings[i], rings[i + 1], vs[i], vs[i + 1], s)
    # Caps: inner top (seen from below) and outer top.
    for ring_i, v in ((0, 0.0), (len(rings) - 1, 0.99)):
        cz = top if not ring_i else top + WALL + 0.002
        centre = bm.verts.new((c.x, c.y + 0.004, cz))
        for s in range(SEGMENTS):
            s2 = (s + 1) % SEGMENTS
            vv = (rings[ring_i][s], rings[ring_i][s2], centre)
            f = bm.faces.new(vv if ring_i else (vv[1], vv[0], vv[2]))
            for loop in f.loops:
                loop[uv].uv = (s / SEGMENTS + 0.5 / SEGMENTS, v)
    bm.normal_update()
    # Make sure the shell faces outward (the order above should already do it).
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    mesh = bpy.data.meshes.new(NAME)
    bm.to_mesh(mesh)
    bm.free()
    for p in mesh.polygons:
        p.use_smooth = False
    obj = bpy.data.objects.new(NAME, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def straw_texture(path, w=128, h=128):
    """Coiled straw plait (rows of chevrons), a dark cloth band and the paler brim underside."""
    rnd = random.Random(SEED + 1)
    px = [0.0] * (w * h * 4)
    straw = (0.86, 0.76, 0.52)
    for y in range(h):
        v = y / h
        for x in range(w):
            n = rnd.uniform(-0.06, 0.06)
            if v >= 0.30:  # plait rows, 6 px tall, chevrons 4 px wide
                row = (y - 38) // 6
                ry = (y - 38) % 6
                cx = (x + (row % 2) * 2) % 8
                chevron = abs(cx - 4) / 4.0
                lit = 0.78 + 0.22 * (1.0 - abs(ry - 2.5 - (chevron - 0.5) * 2) / 3.0)
                if ry == 0:
                    lit *= 0.62  # gap between coils
                if rnd.random() < 0.025:
                    lit *= 0.7  # a darker stalk
                col = [ch * (lit + n) for ch in straw]
                col[2] *= 0.94 + 0.1 * (row % 3 == 0)
            elif v >= 0.18:  # hatband: dark worn cloth
                stripe = 0.9 if (y % 5 == 0) else 1.0
                col = [ch * stripe * (1.0 + n) for ch in (0.23, 0.17, 0.12)]
            else:  # underside: same plait, darker and duller
                ry = y % 6
                cx = (x + (y // 6 % 2) * 2) % 8
                lit = 0.62 + 0.12 * (1.0 - abs(cx - 4) / 4.0) - (0.12 if ry == 0 else 0.0)
                col = [ch * (lit + n) for ch in (0.80, 0.70, 0.50)]
            i = (y * w + x) * 4
            px[i:i + 4] = [max(0.0, min(1.0, col[0])), max(0.0, min(1.0, col[1])), max(0.0, min(1.0, col[2])), 1.0]
    img = bpy.data.images.new(f"{NAME}_tex", width=w, height=h, alpha=False)
    img.pixels[:] = px
    img.filepath_raw = str(path)
    img.file_format = "PNG"
    img.save()
    return img


def inner_radius(radii, levels, s, z):
    """Inner crown wall radius at segment `s` and height `z` (linear between rings)."""
    if z <= levels[0]:
        return radii[s][0], 0, 0.0
    for k in range(len(levels) - 1):
        if z <= levels[k + 1]:
            t = (z - levels[k]) / (levels[k + 1] - levels[k])
            return radii[s][k] * (1 - t) + radii[s][k + 1] * t, k, t
    return radii[s][-1], len(levels) - 2, 1.0


def deficits(pts, c, levels, radii, top):
    """(worst gap, list of (segment, ring, t, deficit)) for points above the band."""
    worst, bad = 9.0, []
    for p in pts:
        if p.z < BAND_Z:
            continue
        if p.z > top:
            worst = min(worst, top - p.z)
            continue
        ang, r = polar(p, c)
        f = (ang % (2 * math.pi)) / (2 * math.pi) * SEGMENTS
        for s in {int(math.floor(f)) % SEGMENTS, int(math.ceil(f)) % SEGMENTS}:
            ri, k, t = inner_radius(radii, levels, s, p.z)
            # the chord between two segments sits inside the corners by cos(pi/N)
            gap = ri * math.cos(math.pi / SEGMENTS) - r
            worst = min(worst, gap)
            if gap < 0.003:
                bad.append((s, k, t, 0.003 - gap))
    return worst, bad


def inflate(pts, c, levels, radii, top):
    for _ in range(20):
        _, bad = deficits(pts, c, levels, radii, top)
        if not bad:
            return
        for s, k, t, d in bad:
            radii[s][k] = max(radii[s][k], radii[s][k] + d * 1.2)
            radii[s][k + 1] = max(radii[s][k + 1], radii[s][k + 1] + d * 1.2 * t)
        for row in radii:
            for k in range(len(row) - 2, -1, -1):
                row[k] = max(row[k], row[k + 1])


def check_clearance(points_by_name, c, levels, radii, top):
    """Smallest distance from head/hair points to the inner crown (negative = pokes through)."""
    for name, pts in points_by_name.items():
        worst, _ = deficits(pts, c, levels, radii, top)
        print(f"[hat-fit] {name}: closest {worst * 1000:.1f} mm {'OK' if worst > 0 else 'POKES THROUGH'}")


def export_glb(hat, arm_obj, tex_img, glb_path):
    pivot = arm_obj.data.bones["head"].head_local.copy()
    socket = bpy.data.objects.new("head_socket", None)
    bpy.context.scene.collection.objects.link(socket)
    socket.location = pivot
    copy = hat.copy()
    copy.data = hat.data.copy()
    copy.name = NAME
    copy.data.transform(Matrix.Translation(-pivot))
    copy.parent = socket
    copy.matrix_parent_inverse = Matrix.Identity(4)
    copy.location = (0, 0, 0)
    bpy.context.scene.collection.objects.link(copy)

    mat = bpy.data.materials.new("chapeu_palha")
    mat.use_nodes = True
    mat.use_backface_culling = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.92
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = tex_img
    tex.interpolation = "Closest"
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    for m in (copy.data, hat.data):
        m.materials.clear()
        m.materials.append(mat)

    bpy.ops.object.select_all(action="DESELECT")
    socket.select_set(True)
    copy.select_set(True)
    bpy.context.view_layer.objects.active = socket
    bpy.ops.export_scene.gltf(filepath=str(glb_path), export_format="GLB", use_selection=True, export_yup=True,
                              export_apply=False, export_texcoords=True, export_normals=True,
                              export_materials="EXPORT", export_image_format="AUTO")
    data = copy.data
    bpy.data.objects.remove(copy, do_unlink=True)
    bpy.data.objects.remove(socket, do_unlink=True)
    bpy.data.meshes.remove(data)
    raw = glb_path.read_bytes()
    magic, version, total = struct.unpack_from("<4sII", raw, 0)
    assert magic == b"glTF" and version == 2 and total == len(raw)
    doc = json.loads(raw[20:20 + struct.unpack_from("<I", raw, 12)[0]])
    print(f"[glb] {glb_path.name} {len(raw)} bytes, images={len(doc.get('images', []))}")


def render_previews(hat, body_obj, arm_obj, out_dir):
    scene = bpy.context.scene
    engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in engines else "BLENDER_EEVEE"
    world = bpy.data.worlds.new("HatWorld")
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.055, 0.064, 0.080, 1.0)
    for lname, loc, energy in [("K", (-2.0, 3.6, 3.4), 4.4), ("F", (2.8, 2.6, 2.2), 2.0), ("R", (0.2, -3.4, 2.8), 2.6)]:
        ld = bpy.data.lights.new(lname, type="SUN")
        ld.energy = energy
        lo = bpy.data.objects.new(lname, ld)
        scene.collection.objects.link(lo)
        lo.rotation_euler = (Vector((0, 0, 1.72)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    cam_data = bpy.data.cameras.new("Cam")
    cam_data.type = "ORTHO"
    cam = bpy.data.objects.new("Cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam

    hair = {}
    for name in UNDER:
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(HAIRS / f"{name}.glb"))
        new = [o for o in bpy.data.objects if o not in before and o.type == "MESH"]
        bpy.context.view_layer.update()
        hair[name] = new
        for o in [o for o in bpy.data.objects if o not in before]:
            o.hide_render = True
    depsgraph = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = True
    hat.hide_render = True
    made = []

    def snap(obj, x, yaw, matrix=None):
        ev = obj.evaluated_get(depsgraph)
        m = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=True, depsgraph=depsgraph)
        if matrix is not None:
            m.transform(matrix)
        o = bpy.data.objects.new("snap", m)
        scene.collection.objects.link(o)
        o.location = (x, 0, 0)
        o.rotation_euler = (0, 0, math.radians(yaw))
        made.append(o)
        return o

    def person(x, yaw, hair_name):
        snap(body_obj, x, yaw)
        if hair_name:
            for h in hair[hair_name]:
                snap(h, x, yaw, h.matrix_world)
        snap(hat, x, yaw)

    def shoot(path, rx, ry, cam_at, look, ortho):
        scene.render.resolution_x, scene.render.resolution_y = rx, ry
        cam.location = cam_at
        cam.rotation_euler = (Vector(look) - Vector(cam_at)).to_track_quat("-Z", "Y").to_euler()
        cam_data.ortho_scale = ortho
        scene.render.filepath = str(path)
        bpy.ops.render.render(write_still=True)
        for o in made:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            bpy.data.meshes.remove(m)
        made.clear()

    for x, yaw, hn in [(0.66, 0, None), (0.22, 40, None),
                       (-0.22, -90, None), (-0.66, -145, None)]:
        person(x, yaw, hn)
    shoot(out_dir / "preview_hat_angles.png", 1600, 640, (0, 3.0, 1.76), (0, 0, 1.72), 1.9)

    person(0.9, -25, None)
    person(-0.1, 150, None)
    iso = snap(hat, -1.1, 25)
    iso.scale = (3, 3, 3)
    iso.location = (-1.1, 0, 0.95 - 1.74 * 3)
    iso2 = snap(hat, -1.1, 25)
    iso2.scale = (3, 3, 3)
    iso2.rotation_euler = (math.radians(180), 0, 0)
    iso2.location = (-1.1, 0, 0.0 + 1.74 * 3 - 0.55 + 0.25)
    shoot(out_dir / "preview_hat_full.png", 1500, 820, (0, 5.0, 1.0), (0, 0, 0.98), 4.0)


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.open_mainfile(filepath=str(CHARTEST_BLEND))
    body = bpy.data.objects["chartest_body"]
    arm = bpy.data.objects["chartest_rig"]
    for t in arm.animation_data.nla_tracks:
        t.mute = True
    arm.animation_data.action = None
    arm["is_female"] = False
    arm["height_m"] = 1.82

    head, hair = head_and_hair_points(body, UNDER + CHECK_ONLY)
    near = [p for p in head if p.z > BAND_Z]
    # Only the head shapes the crown: the engine hides hair under the hat with a mask.
    fit_points = head
    c = Vector((sum(p.x for p in near) / len(near), (min(p.y for p in near) + max(p.y for p in near)) / 2, 0))
    levels, radii, top = fit_crown(fit_points, c)
    print(f"[hat] axis {c.x:.3f},{c.y:.3f} top {top:.3f} band radii {[round(r[0], 3) for r in radii]}")
    hat = build_hat(c, levels, radii, top)
    print(f"[hat] verts={len(hat.data.vertices)} faces={len(hat.data.polygons)}")
    check_clearance({"head": head, **hair}, c, levels, radii, top)

    tex = straw_texture(OUT_DIR / "chapeu_palha_128.png")
    export_glb(hat, arm, tex, OUT_DIR / f"{NAME}.glb")
    render_previews(hat, body, arm, OUT_DIR)
    hat.parent = arm
    hat.parent_type = "BONE"
    hat.parent_bone = "head"
    hat.matrix_parent_inverse = (arm.matrix_world @ arm.pose.bones["head"].matrix).inverted()
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_DIR / "chapeu_palha.blend"))
    print("[done] ChapeuPalha")


if __name__ == "__main__":
    main()
