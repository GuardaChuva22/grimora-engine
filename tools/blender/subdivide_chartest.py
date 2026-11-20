"""Upgrade chartest_body topology with volume-preserving subdivision.

Reads the active hand-corrected runtime character (`chartest.blend` / `chartest.gltf`),
backs up the original 880-tri version into `authoring_archive/before_subdiv/` on first run,
welds split UV-seam vertices without touching loop UVs, reconstructs safe intra-island quads,
applies 1 level of volume-compensated subdivision (protecting hand grip & foot soles),
saves the editable quad-dominant `chartest.blend` and transplants the new skinned mesh
onto `chartest.gltf` while preserving all 42 bones, sockets, and animations bit-for-bit.

Run with Blender 5.2:
    blender --background --python tools/blender/subdivide_chartest.py -- --preview-dir <dir>
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
CHARTEST_DIR = ROOT / "assets/models/chartest"
BACKUP_DIR = CHARTEST_DIR / "authoring_archive/before_subdiv"


def ensure_backup():
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    for name in ("chartest.blend", "chartest.gltf", "chartest.bin", "chartest_walk.bin"):
        src = CHARTEST_DIR / name
        dst = BACKUP_DIR / name
        if src.exists() and not dst.exists():
            shutil.copy2(src, dst)
            print(f"[subdiv] backed up {name} -> authoring_archive/before_subdiv/{name}")


def weld_and_quadrangulate(obj):
    """Weld coincident UV-seam vertices (averaging deform weights) and join safe UV quads."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.verts.ensure_lookup_table()
    deform = bm.verts.layers.deform.verify()
    uv_layer = bm.loops.layers.uv.active

    # 1. Average vertex group weights across coincident spatial vertices before welding.
    clusters = defaultdict(list)
    for v in bm.verts:
        key = (round(v.co.x * 1e5), round(v.co.y * 1e5), round(v.co.z * 1e5))
        clusters[key].append(v)

    for verts in clusters.values():
        if len(verts) < 2:
            continue
        merged_weights = defaultdict(float)
        for v in verts:
            for g_idx, w in v[deform].items():
                merged_weights[g_idx] += w
        total = sum(merged_weights.values())
        if total > 1e-8:
            norm_weights = {g_idx: w / total for g_idx, w in merged_weights.items() if w / total > 1e-5}
            for v in verts:
                v[deform].clear()
                for g_idx, w in norm_weights.items():
                    v[deform][g_idx] = w

    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-4)
    bm.verts.ensure_lookup_table()
    bm.edges.ensure_lookup_table()

    # Close residual right-hand seam splits (< 6.5 mm) left by local distal grip projection,
    # keeping the outer (non-penetrating) position and averaged weights.
    b_verts = [v for v in bm.verts if any(e.is_boundary for e in v.link_edges)]
    if b_verts:
        bmesh.ops.remove_doubles(bm, verts=b_verts, dist=6.5e-3)
        bm.verts.ensure_lookup_table()
        bm.edges.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    boundary_after_weld = sum(1 for e in bm.edges if e.is_boundary)
    print(f"[subdiv] after weld: {len(bm.verts)} verts, {len(bm.faces)} tris, {boundary_after_weld} boundary edges")

    # 2. Convert natural triangle pairs to quads ONLY when UV seam is continuous across the edge
    #    and both triangles have matching UV orientation (prevents cross-island / mirrored UV bleed).
    def uv_winding(face):
        uvs = [l[uv_layer].uv for l in face.loops]
        if len(uvs) < 3:
            return 0.0
        a = uvs[1] - uvs[0]
        b = uvs[2] - uvs[0]
        return a.x * b.y - a.y * b.x

    candidates = []
    for e in bm.edges:
        if len(e.link_faces) != 2:
            continue
        f0, f1 = e.link_faces
        if len(f0.verts) != 3 or len(f1.verts) != 3:
            continue
        # Check face normal angle (< 42 deg)
        if f0.normal.length < 1e-8 or f1.normal.length < 1e-8:
            continue
        if f0.normal.dot(f1.normal) < math.cos(math.radians(42.0)):
            continue
        # Check UV continuity on both shared vertices
        v_a, v_b = e.verts
        uv0_a = next(l[uv_layer].uv for l in f0.loops if l.vert == v_a)
        uv1_a = next(l[uv_layer].uv for l in f1.loops if l.vert == v_a)
        uv0_b = next(l[uv_layer].uv for l in f0.loops if l.vert == v_b)
        uv1_b = next(l[uv_layer].uv for l in f1.loops if l.vert == v_b)
        if (uv0_a - uv1_a).length > 1e-4 or (uv0_b - uv1_b).length > 1e-4:
            continue
        if uv_winding(f0) * uv_winding(f1) <= 0:
            continue

        # Check resulting quad convexity & corner angles
        other0 = next(v for v in f0.verts if v not in (v_a, v_b))
        other1 = next(v for v in f1.verts if v not in (v_a, v_b))
        quad = [other0.co, v_a.co, other1.co, v_b.co]
        n = (f0.normal + f1.normal).normalized()
        good = True
        angles_dev = 0.0
        for i in range(4):
            p_prev = quad[(i - 1) % 4]
            p_curr = quad[i]
            p_next = quad[(i + 1) % 4]
            d1 = (p_prev - p_curr).normalized()
            d2 = (p_next - p_curr).normalized()
            cross = d1.cross(d2)
            # Ensure consistent convexity
            angle = math.degrees(math.acos(max(-1.0, min(1.0, d1.dot(d2)))))
            if angle < 45.0 or angle > 145.0:
                good = False
                break
            angles_dev += abs(angle - 90.0)
        if not good:
            continue
        # Prefer pairing across longest shared hypotenuse with squarest corners
        score = e.calc_length() - 0.0015 * angles_dev
        candidates.append((score, e))

    candidates.sort(key=lambda item: item[0], reverse=True)
    used_faces = set()
    to_join = []
    for _, e in candidates:
        f0, f1 = e.link_faces
        if f0 not in used_faces and f1 not in used_faces:
            used_faces.add(f0)
            used_faces.add(f1)
            to_join.append(e)

    if to_join:
        bmesh.ops.dissolve_edges(bm, edges=to_join, use_verts=False, use_face_split=False)

    bm.faces.ensure_lookup_table()
    quad_count = sum(1 for f in bm.faces if len(f.verts) == 4)
    tri_count = sum(1 for f in bm.faces if len(f.verts) == 3)
    print(f"[subdiv] safe quadrangulation: {quad_count} quads, {tri_count} tris")

    bm.to_mesh(obj.data)
    bm.free()
    obj.data.update()


def subdivide_volume_preserving(obj, body_alpha=0.55, inflate=0.45):
    """Blend Simple (0% shrink) and Catmull-Clark (smooth) subdivision with hand/sole protection."""
    was_smooth = any(p.use_smooth for p in obj.data.polygons)

    def eval_subdiv(subdiv_type):
        copy_obj = obj.copy()
        copy_obj.data = obj.data.copy()
        bpy.context.scene.collection.objects.link(copy_obj)
        # Clear armature modifier during geometry evaluation so bind-space stays pure
        for mod in list(copy_obj.modifiers):
            if mod.type == "ARMATURE":
                copy_obj.modifiers.remove(mod)
        sub = copy_obj.modifiers.new(name="Subdiv", type="SUBSURF")
        sub.levels = 1
        sub.render_levels = 1
        sub.subdivision_type = subdiv_type
        sub.uv_smooth = "NONE"
        sub.boundary_smooth = "PRESERVE_CORNERS"
        bpy.context.view_layer.objects.active = copy_obj
        bpy.ops.object.modifier_apply(modifier=sub.name)
        return copy_obj

    simple_obj = eval_subdiv("SIMPLE")
    smooth_obj = eval_subdiv("CATMULL_CLARK")

    assert len(simple_obj.data.vertices) == len(smooth_obj.data.vertices), "Subdiv vertex count mismatch"

    # Classify bone groups for regional protection (preserve hand grip clearance & foot sole)
    distal_hand_groups = set()
    palm_groups = set()
    head_groups = set()
    for g in simple_obj.vertex_groups:
        n = g.name.lower()
        if any(k in n for k in ("thumb", "index", "finger")):
            distal_hand_groups.add(g.index)
        elif n in ("hand.l", "hand.r"):
            palm_groups.add(g.index)
        elif n in ("head", "neck"):
            head_groups.add(g.index)

    simple_obj.data.calc_normals() if hasattr(simple_obj.data, "calc_normals") else None

    for i, v_out in enumerate(simple_obj.data.vertices):
        co_s = v_out.co.copy()
        co_c = smooth_obj.data.vertices[i].co.copy()
        n = v_out.normal.copy()

        weights = {g.group: g.weight for g in v_out.groups}
        w_distal = sum(w for g_idx, w in weights.items() if g_idx in distal_hand_groups)
        w_palm = sum(w for g_idx, w in weights.items() if g_idx in palm_groups)
        w_head = sum(w for g_idx, w in weights.items() if g_idx in head_groups)

        # Base alpha: slightly higher on cranium/face (0.62) for clean hair/head work,
        # moderate on body (body_alpha=0.55), very conservative on fingers/palm to keep sword grip intact.
        alpha = body_alpha * (1.0 - w_distal - w_palm) + 0.12 * w_distal + 0.20 * w_palm
        if w_head > 0.3:
            alpha = alpha * (1.0 - w_head) + 0.62 * w_head

        # Protect foot soles near z = 0 (Blender Z-up)
        if co_s.z < 0.045:
            sole_t = max(0.0, min(1.0, (co_s.z - 0.006) / 0.039))
            alpha *= 0.10 + 0.90 * sole_t

        delta = (co_c - co_s) * alpha
        # Volume compensation: counteract inward Catmull-Clark shrinkage on convex loops
        inward = delta.dot(n)
        if inward < 0.0 and n.length > 1e-6:
            comp = inflate * (1.0 - 0.7 * (w_distal + w_palm))
            delta -= n * (inward * comp)

        new_co = co_s + delta
        if co_s.z < 0.0025:
            new_co.z = co_s.z
        v_out.co = new_co

    for p in simple_obj.data.polygons:
        p.use_smooth = was_smooth
    simple_obj.data.update()

    # Swap refined mesh data back onto original skinned object (keeps Armature modifier & object identity)
    old_mesh_data = obj.data
    obj.data = simple_obj.data
    obj.data.name = old_mesh_data.name

    bpy.data.objects.remove(simple_obj, do_unlink=True)
    bpy.data.objects.remove(smooth_obj, do_unlink=True)
    if old_mesh_data.users == 0:
        bpy.data.meshes.remove(old_mesh_data)


def transplant_to_runtime_gltf(obj, arm, stage_dir):
    """Transplant only the upgraded skinned mesh onto the baseline chartest.gltf."""
    stage_dir.mkdir(parents=True, exist_ok=True)
    temp_gltf = stage_dir / "temp_mesh.gltf"

    for b in arm.pose.bones:
        b.matrix_basis.identity()
    for t in arm.animation_data.nla_tracks:
        t.mute = True
    arm.animation_data.action = None

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    arm.select_set(True)
    bpy.context.view_layer.objects.active = arm

    bpy.ops.export_scene.gltf(
        filepath=str(temp_gltf),
        export_format="GLTF_SEPARATE",
        use_selection=True,
        export_yup=True,
        export_animations=False,
        export_skins=True,
        export_def_bones=False,
        export_materials="EXPORT",
    )

    for t in arm.animation_data.nla_tracks:
        t.mute = False

    new_doc = json.loads(temp_gltf.read_text(encoding="utf-8"))
    base_doc = json.loads((BACKUP_DIR / "chartest.gltf").read_text(encoding="utf-8"))

    # Copy baseline binary buffers bit-for-bit so all animations & walk/run clips stay 100% untouched
    shutil.copy2(BACKUP_DIR / "chartest.bin", CHARTEST_DIR / "chartest.bin")
    if (BACKUP_DIR / "chartest_walk.bin").exists():
        shutil.copy2(BACKUP_DIR / "chartest_walk.bin", CHARTEST_DIR / "chartest_walk.bin")

    buf_offset = len(base_doc["buffers"])
    view_offset = len(base_doc["bufferViews"])
    acc_offset = len(base_doc["accessors"])

    assert len(new_doc["buffers"]) == 1
    mesh_bin_name = "chartest_mesh_hd.bin"
    shutil.copy2(stage_dir / new_doc["buffers"][0]["uri"], CHARTEST_DIR / mesh_bin_name)
    new_doc["buffers"][0]["uri"] = mesh_bin_name
    base_doc["buffers"].extend(new_doc["buffers"])

    for view in new_doc["bufferViews"]:
        view["buffer"] += buf_offset
    base_doc["bufferViews"].extend(new_doc["bufferViews"])

    for acc in new_doc["accessors"]:
        if "bufferView" in acc:
            acc["bufferView"] += view_offset
    base_doc["accessors"].extend(new_doc["accessors"])

    old_joint_names = [base_doc["nodes"][i]["name"] for i in base_doc["skins"][0]["joints"]]
    new_joint_names = [new_doc["nodes"][i]["name"] for i in new_doc["skins"][0]["joints"]]
    joint_map = [old_joint_names.index(n) for n in new_joint_names]

    processed_joints = set()
    for mesh in new_doc["meshes"]:
        for prim in mesh["primitives"]:
            ai = prim["attributes"].get("JOINTS_0")
            if ai is not None and ai not in processed_joints:
                processed_joints.add(ai)
                acc = base_doc["accessors"][ai + acc_offset]
                view = base_doc["bufferViews"][acc["bufferView"]]
                bpath = CHARTEST_DIR / base_doc["buffers"][view["buffer"]]["uri"]
                data = bytearray(bpath.read_bytes())
                fmt = {5121: "B", 5123: "H"}[acc["componentType"]]
                size = struct.calcsize(fmt)
                stride = view.get("byteStride", size * 4)
                start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
                for row in range(acc["count"]):
                    for c in range(4):
                        off = start + row * stride + c * size
                        val = struct.unpack_from("<" + fmt, data, off)[0]
                        struct.pack_into("<" + fmt, data, off, joint_map[val])
                bpath.write_bytes(data)

            prim["attributes"] = {k: v + acc_offset for k, v in prim["attributes"].items()}
            if "indices" in prim:
                prim["indices"] += acc_offset
            # Keep exact original runtime material index 0 (Chartest-Grimora-v02-128.png nearest)
            prim["material"] = 0

    base_doc["meshes"] = new_doc["meshes"]
    base_doc["asset"]["generator"] = "Grimora tools/blender/subdivide_chartest.py"
    (CHARTEST_DIR / "chartest.gltf").write_text(json.dumps(base_doc, indent=1) + "\n", encoding="utf-8")


def setup_textured_material(obj):
    """Ensure Eevee/Workbench renders Chartest-Grimora-v02-128.png with crisp PSX Closest filtering."""
    tex_path = CHARTEST_DIR / "Chartest-Grimora-v02-128.png"
    img = bpy.data.images.load(str(tex_path), check_existing=True)
    mat = bpy.data.materials.new("Preview_PSX")
    mat.use_nodes = True
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()

    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.88
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.08
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Closest"

    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])

    obj.data.materials.clear()
    obj.data.materials.append(mat)


def render_comparison(before_blend, after_blend, out_dir):
    """Render side-by-side Before (880 tris) vs After (~3100 tris) textured & wireframe views."""
    out_dir.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene

    # Append Before & After meshes
    def load_body(blend_path, label, x_offset):
        with bpy.data.libraries.load(str(blend_path), link=False) as (data_from, data_to):
            data_to.objects = [n for n in data_from.objects if n == "chartest_body"]
        obj = data_to.objects[0]
        obj.name = label
        scene.collection.objects.link(obj)
        obj.modifiers.clear()
        obj.parent = None
        obj.matrix_world.identity()
        obj.location.x = x_offset
        return obj

    before_obj = load_body(before_blend, "Before_880", 0.46)
    after_obj = load_body(after_blend, "After_HD", -0.46)
    setup_textured_material(before_obj)
    setup_textured_material(after_obj)

    # Studio lights for clean Eevee render (character faces +Y in Blender Z-up)
    world = bpy.data.worlds.new("PreviewWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.055, 0.062, 0.078, 1.0)
    bg.inputs[1].default_value = 0.95

    for name, loc, energy, color in [
        ("Key", (-1.8, 2.6, 2.5), 4.2, (1.0, 0.97, 0.92)),
        ("Fill", (2.2, 2.0, 1.6), 2.0, (0.82, 0.88, 1.0)),
        ("Rim", (0.0, -2.4, 2.2), 2.2, (0.95, 0.95, 1.0)),
    ]:
        ldata = bpy.data.lights.new(name, type="SUN")
        ldata.energy = energy
        ldata.color = color
        lobj = bpy.data.objects.new(name, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.0)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()

    cam_data = bpy.data.cameras.new("Cam")
    cam = bpy.data.objects.new("Cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam

    # 1. Textured renders (Full Body Front 3/4 + Head/Shoulders Close-up)
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"

    # Full body (left on screen = Before_880 at +X when looking from +Y towards -Y!)
    scene.render.resolution_x = 1360
    scene.render.resolution_y = 980
    cam_data.type = "PERSP"
    cam_data.lens = 46
    cam.location = (-0.28, 2.65, 1.02)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.93)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out_dir / "compare_body_textured.png")
    bpy.ops.render.render(write_still=True)

    # Closer spacing for Head/Face/Shoulders comparison
    before_obj.location.x = 0.22
    after_obj.location.x = -0.22
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 760
    cam_data.lens = 58
    cam.location = (-0.16, 1.12, 1.63)
    cam.rotation_euler = (Vector((0.0, 0.0, 1.58)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out_dir / "compare_head_textured.png")
    bpy.ops.render.render(write_still=True)

    # 2. Clay + Wireframe overlay comparison so quad topology is crystal clear
    clay = bpy.data.materials.new("Clay")
    clay.use_nodes = True
    clay.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.60, 0.56, 0.52, 1.0)
    clay.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.72

    wire_mat = bpy.data.materials.new("Wire")
    wire_mat.use_nodes = True
    wire_mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.06, 0.08, 0.11, 1.0)
    wire_mat.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.9

    for o in (before_obj, after_obj):
        o.data.materials.clear()
        o.data.materials.append(clay)
        o.data.materials.append(wire_mat)
        wmod = o.modifiers.new("WireOverlay", type="WIREFRAME")
        wmod.thickness = 0.0018
        wmod.use_replace = False
        wmod.material_offset = 1

    # Head/Shoulders wireframe first (already at +/- 0.22)
    scene.render.filepath = str(out_dir / "compare_head_wireframe.png")
    bpy.ops.render.render(write_still=True)

    # Full body front wireframe
    before_obj.location.x = 0.46
    after_obj.location.x = -0.46
    scene.render.resolution_x = 1360
    scene.render.resolution_y = 980
    cam_data.lens = 46
    cam.location = (-0.28, 2.65, 1.02)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.93)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out_dir / "compare_body_wireframe.png")
    bpy.ops.render.render(write_still=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--preview-dir", type=Path, required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else [])

    ensure_backup()

    bpy.ops.wm.open_mainfile(filepath=str(BACKUP_DIR / "chartest.blend"))
    bpy.context.preferences.filepaths.save_version = 0

    obj = next(o for o in bpy.context.scene.objects if o.type == "MESH" and o.name == "chartest_body")
    arm = next(o for o in bpy.context.scene.objects if o.type == "ARMATURE" and o.name == "chartest_rig")

    weld_and_quadrangulate(obj)
    subdivide_volume_preserving(obj, body_alpha=0.55, inflate=0.45)

    quads = sum(1 for p in obj.data.polygons if len(p.vertices) == 4)
    tris = sum(1 for p in obj.data.polygons if len(p.vertices) == 3)
    total_tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
    print(f"[subdiv] final chartest_body: {len(obj.data.vertices)} verts, {len(obj.data.polygons)} polys ({quads} quads, {tris} tris -> {total_tris} runtime tris)")

    # Save updated editable chartest.blend (quad-dominant for modeling hair/clothes/morphs!)
    bpy.ops.wm.save_as_mainfile(filepath=str(CHARTEST_DIR / "chartest.blend"), compress=True)

    # Transplant onto runtime chartest.gltf keeping all existing runtime animations untouched
    transplant_to_runtime_gltf(obj, arm, args.preview_dir / "stage")

    # Render Before vs After comparison sheets
    render_comparison(BACKUP_DIR / "chartest.blend", CHARTEST_DIR / "chartest.blend", args.preview_dir)
    print("[subdiv] complete!")


if __name__ == "__main__":
    main()
