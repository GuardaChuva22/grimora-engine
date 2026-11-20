"""Build Grimora PSX Universal Auto-Fit Over-Garment Aprons in a single fast Blender 5.2 pass (v2 — Zero-Clip Envelope):
1. AventalCouroFerreiro.glb — Heavy Blacksmith Leather Apron (broad squared riveted chest bib, X-back shoulder straps
   with forged iron cross-plate, 360° forged-buckle waist belt, right-hip hammer loop, chest & thigh pockets, split-leg
   knee-length rawhide shield).
2. AventalPanoTaverna.glb — Tavern / Cook / Healer Cloth Apron (curved sweetheart pinafore bib, collar-hugging halter
   neck loop, pleated sage-olive waist sash with 3D back bow & tails, rounded gathered front skirt with folk border,
   divided double herb/tavern pocket, and tucked front-left hand-towel).
"""

from collections import defaultdict
import math
import pathlib
import sys

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

SCRATCH_DIR = pathlib.Path(__file__).resolve().parent
if str(SCRATCH_DIR) not in sys.path:
    sys.path.insert(0, str(SCRATCH_DIR))

from grimora_cloth_kit import (
    TECIDO_GROSSO_PNG,
    autofit_garment_to_chartest,
    bell,
    clamp,
    enforce_mesh_clearance_over_bvh,
    export_uv_overlay,
    extrude_closed_rim_inward,
    open_clean_chartest,
    ordered_boundary_loop,
    set_relaxed_preview_pose,
    smoothstep,
    verify_garment_and_export_glb,
)

REPO_ROOT = pathlib.Path(r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE")
CLOTHES_DIR = REPO_ROOT / "assets" / "models" / "chartest" / "clothes"
TEX_DIR = REPO_ROOT / "assets" / "textures"
COURO_DARK_PNG = TEX_DIR / "couro1.png"
COURO_LIGHT_PNG = TEX_DIR / "couro_claro.png"

BRAIN_DIR = pathlib.Path(r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out")
OUT_LEATHER = BRAIN_DIR / "accessory_leather_apron"
OUT_CLOTH = BRAIN_DIR / "accessory_cloth_apron"


# =============================================================================
# COMPOSITE UNDER-GARMENT ENVELOPE & MULTI-GARMENT MORPH LOCKSTEP
# =============================================================================
def import_reference_garments(glb_names: list[str]) -> tuple[list[bpy.types.Object], list[bpy.types.Object]]:
    """Temporarily import approved garment GLBs and return (mesh_objects, all_imported_objects)."""
    meshes = []
    all_imported = []
    for gname in glb_names:
        gpath = CLOTHES_DIR / gname
        if not gpath.exists():
            continue
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(gpath))
        added = [o for o in bpy.data.objects if o not in before]
        all_imported.extend(added)
        for o in added:
            o.hide_render = True
            if o.type == "MESH":
                meshes.append(o)
    return meshes, all_imported


def build_composite_under_bvh(
    body_obj: bpy.types.Object,
    garment_objs: list[bpy.types.Object],
    morph_weights: dict[str, float] | None = None,
    exclude_arms_x: float = 0.205,
    exclude_head_z: float = 1.540,
) -> tuple[BVHTree, list[Vector]]:
    """Build a unified BVHTree of chartest_body + underlying clothes (at Basis or arbitrary morph_weights dict),
    strictly excluding all arm/hand/finger geometry (which reaches z ~ 0.76 in A-pose!) and upper head/chin.
    """
    verts_all: list[Vector] = []
    tris_all: list[tuple[int, int, int]] = []
    arm_vgs = {
        "upper_arm.L", "forearm.L", "hand.L", "thumb.L", "thumb_02_l_013", "thumb_03_l_014",
        "index_01_l_015", "index_02_l_016", "index_03_l_017", "fingers.L", "fingertips.L", "fingers_03_l_020",
        "upper_arm.R", "forearm.R", "hand.R", "thumb.R", "thumb_02_r_028", "thumb_03_r_029",
        "index_01_r_030", "index_02_r_031", "index_03_r_032", "fingers.R", "fingertips.R", "fingers_03_r_035",
    }

    for obj in [body_obj] + list(garment_objs):
        me = obj.data
        me.calc_loop_triangles()
        vg_names = [vg.name for vg in obj.vertex_groups]
        arm_g_indices = {i for i, n in enumerate(vg_names) if n in arm_vgs}

        is_arm_vert = []
        for v in me.vertices:
            aw = sum(g.weight for g in v.groups if g.group in arm_g_indices)
            is_arm_vert.append(aw > 0.30)

        sk = me.shape_keys
        if sk and sk.key_blocks:
            basis_d = sk.key_blocks[0].data
            coords = [basis_d[i].co.copy() for i in range(len(me.vertices))]
            if morph_weights:
                for kname, kval in morph_weights.items():
                    if abs(kval) > 1e-6 and kname in sk.key_blocks:
                        kb_d = sk.key_blocks[kname].data
                        for i in range(len(coords)):
                            coords[i] += (kb_d[i].co - basis_d[i].co) * kval
        else:
            coords = [v.co.copy() for v in me.vertices]

        base_offset = len(verts_all)
        verts_all.extend(coords)

        for lt in me.loop_triangles:
            i0, i1, i2 = lt.vertices
            if is_arm_vert[i0] or is_arm_vert[i1] or is_arm_vert[i2]:
                continue
            p0, p1, p2 = coords[i0], coords[i1], coords[i2]
            cx = (p0.x + p1.x + p2.x) / 3.0
            cz = (p0.z + p1.z + p2.z) / 3.0
            if abs(cx) > exclude_arms_x and cz > 0.65:
                continue
            if cz > exclude_head_z:
                continue
            tris_all.append((base_offset + i0, base_offset + i1, base_offset + i2))

    return BVHTree.FromPolygons(verts_all, tris_all), verts_all


def outermost_hit(bvh: BVHTree, orig: Vector, hd: Vector, max_dist: float = 0.34):
    """Return (hit, normal) of the OUTERMOST layer along a ray (body -> pants -> shirt -> skirt), not just the first."""
    best_hit, best_nrm = None, None
    cur = orig.copy()
    remaining = max_dist
    for _ in range(12):
        hit, nrm, _, dist = bvh.ray_cast(cur, hd, remaining)
        if hit is None:
            break
        best_hit, best_nrm = hit, nrm
        cur = hit + hd * 0.0006
        remaining -= dist + 0.0006
        if remaining <= 0.0:
            break
    return best_hit, best_nrm


def raycast_torso_point(under_bvh: BVHTree, ang: float, z_lvl: float, y_center: float, offset: float, max_dist: float = 0.34) -> Vector:
    """Shoot horizontal ray from (0, y_center, z_lvl) at angle `ang` (0=+Y front, +pi/2=+X right, pi=-Y back)
    and land `offset` beyond the OUTERMOST garment layer."""
    hd = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
    orig = Vector((0.0, y_center, z_lvl))
    hit, _ = outermost_hit(under_bvh, orig, hd, max_dist)
    if hit is not None:
        return hit + hd * offset
    return orig + hd * (0.145 + offset)


def add_extruded_patch_with_rim(
    bm: bmesh.types.BMesh,
    grid_pts: list[list[Vector]],
    rim_thick: float,
    sink_depth: float,
    flip_winding: bool = False,
) -> tuple[list[bmesh.types.BMFace], list[bmesh.types.BMFace], list[bmesh.types.BMFace], set[bmesh.types.BMVert]]:
    """Create a quad grid patch (rows x cols) and extrude its outer boundary inward for thick 3D leather/cloth edges."""
    n_r = len(grid_pts)
    n_c = len(grid_pts[0])
    v_grid = [[bm.verts.new(pt) for pt in row] for row in grid_pts]
    front_faces = []
    for r in range(n_r - 1):
        for c in range(n_c - 1):
            v00, v01 = v_grid[r][c], v_grid[r][c + 1]
            v10, v11 = v_grid[r + 1][c], v_grid[r + 1][c + 1]
            verts = (v00, v10, v11, v01) if flip_winding else (v00, v01, v11, v10)
            front_faces.append(bm.faces.new(verts))

    bm.normal_update()
    b_edges = [e for f in front_faces for e in f.edges if e.is_boundary]
    loop = ordered_boundary_loop(b_edges)

    e_set = {(loop[i], loop[(i + 1) % len(loop)]) for i in range(len(loop))}
    match_dir = 0
    for f in front_faces:
        fl = list(f.loops)
        for i in range(len(fl)):
            va, vb = fl[i].vert, fl[(i + 1) % len(fl)].vert
            if (va, vb) in e_set:
                match_dir += 1
            elif (vb, va) in e_set:
                match_dir -= 1
    if match_dir < 0:
        loop = list(reversed(loop))

    in_dirs = []
    for v in loop:
        fn = sum((f.normal for f in v.link_faces), Vector((0.0, 0.0, 0.0)))
        in_d = (-fn.normalized()) if fn.length > 1e-5 else Vector((0.0, -1.0, 0.0))
        in_dirs.append(in_d)

    rf, sf, rv, dv = extrude_closed_rim_inward(bm, loop, in_dirs, rim_thick=rim_thick, sink_depth=sink_depth)
    interior = set(rv) | set(dv)
    return front_faces, rf, sf, interior


def sanitize_apron_weights(garment_obj: bpy.types.Object):
    """Strip arm/hand/finger and head bone weights from aprons so arm swings or head turns NEVER pull straps/towels!"""
    forbidden_to_target = {
        "head": "neck",
        "upper_arm.L": "chest",
        "forearm.L": "hips",
        "hand.L": "hips",
        "thumb.L": "hips",
        "thumb_02_l_013": "hips",
        "thumb_03_l_014": "hips",
        "index_01_l_015": "hips",
        "index_02_l_016": "hips",
        "index_03_l_017": "hips",
        "fingers.L": "hips",
        "fingertips.L": "hips",
        "fingers_03_l_020": "hips",
        "upper_arm.R": "chest",
        "forearm.R": "hips",
        "hand.R": "hips",
        "thumb.R": "hips",
        "thumb_02_r_028": "hips",
        "thumb_03_r_029": "hips",
        "index_01_r_030": "hips",
        "index_02_r_031": "hips",
        "index_03_r_032": "hips",
        "fingers.R": "hips",
        "fingertips.R": "hips",
        "fingers_03_r_035": "hips",
        "weapon.R": "hips",
    }
    vg_map = {vg.name: vg for vg in garment_obj.vertex_groups}
    idx_to_name = {vg.index: vg.name for vg in garment_obj.vertex_groups}

    for v in garment_obj.data.vertices:
        w_dict = defaultdict(float)
        changed = False
        for g in v.groups:
            gname = idx_to_name[g.group]
            if gname in forbidden_to_target and g.weight > 1e-6:
                tgt = "chest" if (v.co.z > 1.15 and gname != "head") else forbidden_to_target[gname]
                w_dict[tgt] += g.weight
                changed = True
            elif g.weight > 1e-6:
                w_dict[gname] += g.weight
        if changed:
            for vg in garment_obj.vertex_groups:
                vg.remove([v.index])
            tot = sum(w_dict.values())
            if tot > 1e-6:
                for gname, gw in w_dict.items():
                    vg_map[gname].add([v.index], gw / tot, "REPLACE")


def lockstep_over_clothes_morphs(
    apron_obj: bpy.types.Object,
    body_obj: bpy.types.Object,
    under_garments: list[bpy.types.Object],
    interior_indices: set[int],
):
    """LOCKSTEP ENVELOPE MORPH TRANSFER:
    For every one of the 24 shape keys, each apron vertex inspects all nearby surfaces in
    `[body_obj] + under_garments` (within 6.5cm) and ensures its outward normal projection
    `delta.dot(n_out)` is AT LEAST `max(source_delta.dot(n_out))`!
    - When clothes expand outward (+25mm on Female_Breasts or Body_Heavy_Fat), the apron expands >= +26mm!
    - When body shrinks inward (-12mm on Feminine_Body while shirt only shrinks -7mm), the apron shrinks at most -6.5mm!
    This mathematically guarantees positive clearance under ANY simultaneous multi-slider preset!
    """
    ap_mesh = apron_obj.data
    ap_sk = ap_mesh.shape_keys
    n_ap = len(ap_mesh.vertices)
    ap_base = [ap_sk.key_blocks[0].data[i].co.copy() for i in range(n_ap)]

    ap_adj = defaultdict(set)
    for poly in ap_mesh.polygons:
        vs = poly.vertices
        for i in range(len(vs)):
            a, b = vs[i], vs[(i + 1) % len(vs)]
            ap_adj[a].add(b)
            ap_adj[b].add(a)

    # Prebuild clean torso/leg BVHs + barycentric mappings for body + each underlying garment!
    arm_vgs = {
        "upper_arm.L", "forearm.L", "hand.L", "thumb.L", "thumb_02_l_013", "thumb_03_l_014",
        "index_01_l_015", "index_02_l_016", "index_03_l_017", "fingers.L", "fingertips.L", "fingers_03_l_020",
        "upper_arm.R", "forearm.R", "hand.R", "thumb.R", "thumb_02_r_028", "thumb_03_r_029",
        "index_01_r_030", "index_02_r_031", "index_03_r_032", "fingers.R", "fingertips.R", "fingers_03_r_035",
    }
    sources_info = []
    for src_obj in [body_obj] + list(under_garments):
        me = src_obj.data
        sk = me.shape_keys
        if not sk or not sk.key_blocks:
            continue
        me.calc_loop_triangles()
        vg_names = [vg.name for vg in src_obj.vertex_groups]
        arm_g_idx = {i for i, n in enumerate(vg_names) if n in arm_vgs}
        is_arm = [sum(g.weight for g in v.groups if g.group in arm_g_idx) > 0.30 for v in me.vertices]
        b_coords = [sk.key_blocks[0].data[i].co.copy() for i in range(len(me.vertices))]
        tris = []
        for lt in me.loop_triangles:
            i0, i1, i2 = lt.vertices
            if is_arm[i0] or is_arm[i1] or is_arm[i2]:
                continue
            cz = (b_coords[i0].z + b_coords[i1].z + b_coords[i2].z) / 3.0
            if cz > 1.600:
                continue
            tris.append((i0, i1, i2))
        if not tris:
            continue
        bvh = BVHTree.FromPolygons(b_coords, tris)

        # Map every apron vertex to this source if within 6.5cm
        v_map = []
        for p in ap_base:
            loc, nrm, f_idx, dist = bvh.find_nearest(p)
            if loc is None or dist > 0.065:
                v_map.append(None)
                continue
            i0, i1, i2 = tris[f_idx]
            a, b, c = b_coords[i0], b_coords[i1], b_coords[i2]
            v0, v1, v2 = b - a, c - a, loc - a
            d00, d01, d11 = v0.dot(v0), v0.dot(v1), v1.dot(v1)
            d20, d21 = v2.dot(v0), v2.dot(v1)
            denom = d00 * d11 - d01 * d01
            if abs(denom) > 1e-12:
                wb = clamp((d11 * d20 - d01 * d21) / denom, 0.0, 1.0)
                wc = clamp((d00 * d21 - d01 * d20) / denom, 0.0, 1.0)
                if wb + wc > 1.0:
                    s = wb + wc
                    wb /= s
                    wc /= s
                wa = 1.0 - wb - wc
            else:
                wa, wb, wc = 1.0, 0.0, 0.0
            n_o = nrm.normalized() if (nrm is not None and nrm.length > 1e-5) else Vector((0.0, 1.0, 0.0))
            v_map.append((i0, i1, i2, wa, wb, wc, n_o))
        sources_info.append((src_obj, b_coords, v_map))

    # Outward reference normal per apron vertex (from composite under-BVH!)
    comp_bvh, _ = build_composite_under_bvh(body_obj, under_garments, exclude_arms_x=0.205, exclude_head_z=1.600)
    ap_out_nrms = []
    for p in ap_base:
        loc, nrm, _, _ = comp_bvh.find_nearest(p)
        if nrm is not None and nrm.length > 1e-5:
            # Make sure nrm points from loc toward p!
            n_u = nrm.normalized()
            if (p - loc).dot(n_u) < -0.005:
                n_u = Vector((p.x, p.y + 0.045, 0.0)).normalized()
            ap_out_nrms.append(n_u)
        else:
            ap_out_nrms.append(Vector((p.x, p.y + 0.045, 0.0)).normalized())

    # Update every shape key on the apron to strictly dominate all nearby underlying garments!
    for kb in ap_sk.key_blocks[1:]:
        kname = kb.name
        if kname.startswith(("Head", "Jaw", "Face")):
            continue

        src_key_deltas = []
        for src_obj, b_coords, v_map in sources_info:
            skb = src_obj.data.shape_keys.key_blocks.get(kname)
            if skb is None:
                continue
            kd = [skb.data[i].co - b_coords[i] for i in range(len(b_coords))]
            src_key_deltas.append((kd, v_map))

        new_deltas = []
        min_proj_req = []
        for vi in range(n_ap):
            cur_d = kb.data[vi].co - ap_base[vi]
            n_out = ap_out_nrms[vi]
            best_proj = cur_d.dot(n_out)
            best_vec = cur_d.copy()

            for kd, v_map in src_key_deltas:
                m = v_map[vi]
                if m is None:
                    continue
                i0, i1, i2, wa, wb, wc, _ = m
                sd = kd[i0] * wa + kd[i1] * wb + kd[i2] * wc
                proj = sd.dot(n_out)
                # Add +6% cushion when expanding outward, or shrink 6% less when contracting inward!
                req_proj = proj * 1.06 if proj >= 0.0 else proj * 0.92
                if req_proj > best_proj:
                    best_proj = req_proj
                    best_vec = sd + n_out * (req_proj - proj)

            new_deltas.append(best_vec)
            min_proj_req.append(best_proj)

        # Smooth deltas across apron neighbors while strictly preserving `d.dot(n_out) >= min_proj_req`!
        for _ in range(3):
            nxt = []
            for vi in range(n_ap):
                nbs = ap_adj[vi]
                sm = new_deltas[vi].lerp(sum((new_deltas[nb] for nb in nbs), Vector()) / len(nbs), 0.35) if nbs else new_deltas[vi]
                if vi not in interior_indices:
                    n_out = ap_out_nrms[vi]
                    cp = sm.dot(n_out)
                    if cp < min_proj_req[vi]:
                        sm = sm + n_out * (min_proj_req[vi] - cp)
                nxt.append(sm)
            new_deltas = nxt

        for vi in range(n_ap):
            kb.data[vi].co = ap_base[vi] + new_deltas[vi]

    # Finally, explicitly verify & enforce composite clearance on combined multi-slider presets too!
    combo_checks = [
        ("Female_Breasts", {"Feminine_Body": 1.0, "Female_Breasts": 2.0}, "Female_Breasts", 2.0),
        ("Body_Muscle", {"Body_Muscle": 1.0, "Shoulders_Width": 0.55}, "Body_Muscle", 1.0),
        ("Body_Heavy_Fat", {"Body_Heavy_Fat": 1.0, "Abdomen_Belly": 0.65, "Hips_Width": 0.45}, "Body_Heavy_Fat", 1.0),
    ]
    for target_key, weights, divide_key, divide_val in combo_checks:
        c_bvh, _ = build_composite_under_bvh(body_obj, under_garments, morph_weights=weights, exclude_arms_x=0.205, exclude_head_z=1.600)
        bm_c = bmesh.new()
        bm_c.from_mesh(ap_mesh)
        bm_c.verts.ensure_lookup_table()
        for vi, v in enumerate(bm_c.verts):
            pos = ap_base[vi].copy()
            for wk, wv in weights.items():
                pos += (ap_sk.key_blocks[wk].data[vi].co - ap_base[vi]) * wv
            v.co = pos
        before_co = [v.co.copy() for v in bm_c.verts]
        skip_bm = {bm_c.verts[vi] for vi in interior_indices if vi < len(bm_c.verts)}
        enforce_mesh_clearance_over_bvh(bm_c, c_bvh, skip_verts=skip_bm, min_vert_gap=0.0055, min_face_gap=0.0042)
        tkb = ap_sk.key_blocks[divide_key]
        for vi, v in enumerate(bm_c.verts):
            corr = v.co - before_co[vi]
            if corr.length > 1e-6:
                tkb.data[vi].co += corr / divide_val
        bm_c.free()


def audit_over_outfit_clearance(
    apron_obj: bpy.types.Object,
    body_obj: bpy.types.Object,
    arm_obj: bpy.types.Object,
    outfit_objs: list[bpy.types.Object],
    interior_indices: set[int],
    label_prefix: str,
):
    """Audit evaluated apron mesh against equipped underlying clothes (excluding arm sleeves!) across 5 body types."""
    for o in outfit_objs:
        if "Armature" in o.modifiers:
            o.modifiers["Armature"].object = arm_obj
    deps = bpy.context.evaluated_depsgraph_get()
    arm_vgs = {
        "upper_arm.L", "forearm.L", "hand.L", "thumb.L", "thumb_02_l_013", "thumb_03_l_014",
        "index_01_l_015", "index_02_l_016", "index_03_l_017", "fingers.L", "fingertips.L", "fingers_03_l_020",
        "upper_arm.R", "forearm.R", "hand.R", "thumb.R", "thumb_02_r_028", "thumb_03_r_029",
        "index_01_r_030", "index_02_r_031", "index_03_r_032", "fingers.R", "fingertips.R", "fingers_03_r_035",
    }

    presets = [
        ("Male_Base", {"is_female": False, "breasts": 0.0, "glutes": 0.0, "h": 1.82, "sk": {}}),
        ("Female_Std", {"is_female": True, "breasts": 1.10, "glutes": 0.90, "h": 1.74, "sk": {}}),
        ("Female_MaxCurvy", {"is_female": True, "breasts": 2.0, "glutes": 2.0, "h": 1.74, "sk": {}}),
        ("Muscular_Tall", {"is_female": False, "breasts": 0.0, "glutes": 0.3, "h": 1.94, "sk": {"Body_Muscle": 1.0, "Shoulders_Width": 0.55, "Arms_Upper": 0.55}}),
        ("Heavy_Robust", {"is_female": False, "breasts": 0.0, "glutes": 0.55, "h": 1.78, "sk": {"Body_Heavy_Fat": 1.0, "Abdomen_Belly": 0.65, "Hips_Width": 0.45}}),
    ]

    for pname, pr in presets:
        for kb in body_obj.data.shape_keys.key_blocks[1:]:
            kb.value = 0.0
        arm_obj["height_m"] = pr["h"]
        arm_obj["is_female"] = pr["is_female"]
        arm_obj["breasts_size"] = pr["breasts"]
        arm_obj["glutes_size"] = pr["glutes"]
        for k, v in pr["sk"].items():
            body_obj.data.shape_keys.key_blocks[k].value = v

        for o in outfit_objs:
            if o.data.shape_keys:
                for kb in o.data.shape_keys.key_blocks[1:]:
                    kb.value = body_obj.data.shape_keys.key_blocks[kb.name].value if kb.name in body_obj.data.shape_keys.key_blocks else 0.0
                o.data.shape_keys.key_blocks["Feminine_Body"].value = 1.0 if pr["is_female"] else 0.0
                o.data.shape_keys.key_blocks["Female_Breasts"].value = pr["breasts"] if pr["is_female"] else 0.0
                o.data.shape_keys.key_blocks["Glutes_Mass"].value = pr["glutes"]
                h = pr["h"]
                o.data.shape_keys.key_blocks["_Height_Short_Comp"].value = clamp((1.82 - h) / (1.82 - 0.50), 0.0, 1.0) if h < 1.82 else 0.0
                o.data.shape_keys.key_blocks["_Height_Tall_Comp"].value = clamp((h - 1.82) / (2.50 - 1.82), 0.0, 1.0) if h > 1.82 else 0.0
            o.update_tag()

        arm_obj.update_tag()
        body_obj.update_tag()
        apron_obj.update_tag()
        deps.update()

        v_all, t_all = [], []
        for o in outfit_objs:
            vg_names = [vg.name for vg in o.vertex_groups]
            arm_g_idx = {i for i, n in enumerate(vg_names) if n in arm_vgs}
            is_arm = [sum(g.weight for g in v.groups if g.group in arm_g_idx) > 0.30 for v in o.data.vertices]
            ev_o = o.evaluated_get(deps)
            me_o = ev_o.to_mesh()
            me_o.calc_loop_triangles()
            mw = ev_o.matrix_world
            coords = [mw @ v.co for v in me_o.vertices]
            off = len(v_all)
            v_all.extend(coords)
            for lt in me_o.loop_triangles:
                i0, i1, i2 = lt.vertices
                if i0 < len(is_arm) and (is_arm[i0] or is_arm[i1] or is_arm[i2]):
                    continue
                t_all.append((off + i0, off + i1, off + i2))
            ev_o.to_mesh_clear()

        u_bvh = BVHTree.FromPolygons(v_all, t_all)
        ev_a = apron_obj.evaluated_get(deps)
        me_a = ev_a.to_mesh()
        mw_a = ev_a.matrix_world
        deep_clips = 0
        min_gap_mm = 999.0
        clip_locs = []
        for vi, v in enumerate(me_a.vertices):
            if vi in interior_indices:
                continue
            wp = mw_a @ v.co
            loc, nrm, _, dist = u_bvh.find_nearest(wp)
            if loc is not None and nrm is not None and dist < 0.055:
                # Only evaluate outward-facing clothes faces (ignore inward-facing collar/hem interior rims!)
                out_ref = Vector((wp.x, wp.y + 0.045, 0.0))
                if nrm.dot(out_ref) < 0.0:
                    continue
                sg = (wp - loc).dot(nrm) * 1000.0
                if sg < min_gap_mm:
                    min_gap_mm = sg
                if sg < -1.0:
                    deep_clips += 1
                    clip_locs.append(f"({wp.x:+.3f},{wp.y:+.3f},{wp.z:.3f}:{sg:+.0f})")
        ev_a.to_mesh_clear()
        print(f"  [outfit-audit {label_prefix} {pname:15s}] min_gap_over_clothes={min_gap_mm:+.2f}mm (deep_clips={deep_clips}) {' '.join(clip_locs[:8])}")

    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"], arm_obj["is_female"], arm_obj["breasts_size"], arm_obj["glutes_size"] = 1.82, False, 0.0, 0.0
    deps.update()


# =============================================================================
# 1. AVENTAL DE COURO (FERREIRO) — TEXTURE + MESH
# =============================================================================
def generate_leather_apron_texture(dark_png: pathlib.Path, light_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    im_d = bpy.data.images.load(str(dark_png), check_existing=True)
    im_l = bpy.data.images.load(str(light_png), check_existing=True)
    w, h = 128, 128
    dw, dh = im_d.size
    lw, lh = im_l.size
    dp = list(im_d.pixels[:])
    lp = list(im_l.pixels[:])
    out = [0.0] * (w * h * 4)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            u = (x + 0.5) / w
            idx = (y * w + x) * 4

            di = ((y % dh) * dw + (x % dw)) * 4
            dr, dg, db = dp[di], dp[di + 1], dp[di + 2]

            li = ((y % lh) * lw + (x % lw)) * 4
            lr, lg, lb = lp[li], lp[li + 1], lp[li + 2]

            if v < 0.085:
                out[idx : idx + 4] = [dr * 0.36, dg * 0.34, db * 0.32, 1.0]
            elif v < 0.215:
                grain = 0.299 * dr + 0.587 * dg + 0.114 * db
                hammer = 0.85 + 0.30 * math.sin(x * 0.9) * math.cos(y * 0.9)
                m = clamp(0.27 + grain * 0.62 * hammer, 0.22, 0.62)
                if y in (13, 26) or x in (6, 121):
                    m = clamp(m * 1.32, 0.0, 0.80)
                out[idx : idx + 4] = [m * 0.95, m * 0.98, m * 1.04, 1.0]
            elif v < 0.375:
                r, g, b = lr * 0.92 + 0.02, lg * 0.86 + 0.01, lb * 0.78
                if y in (30, 45) and ((x // 2) % 2 == 0):
                    r, g, b = clamp(r * 1.48 + 0.12), clamp(g * 1.44 + 0.10), clamp(b * 1.35 + 0.08)
                if y == 38 and (x % 16 == 8):
                    r, g, b = 0.12, 0.11, 0.11
                elif abs(y - 38) <= 1 and abs((x % 16) - 8) <= 1 and (x % 32 == 8):
                    r, g, b = 0.36, 0.37, 0.40
                out[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            else:
                soot = 0.72 + 0.28 * smoothstep(0.38, 0.68, v)
                wear = 1.0 + 0.12 * bell(u, 0.50, 0.24) * bell(v, 0.68, 0.18)
                r = dr * 1.06 * soot * wear
                g = dg * 1.02 * soot * wear
                b = db * 0.98 * soot * wear
                near_border = (x in (3, 124) and 51 <= y <= 124) or (y in (51, 124) and 3 <= x <= 124)
                if near_border and (((x + y) // 2) % 2 == 0):
                    r, g, b = clamp(r + 0.28), clamp(g + 0.23), clamp(b + 0.16)
                for rx, ry in [(7, 55), (120, 55), (7, 120), (120, 120)]:
                    if abs(x - rx) + abs(y - ry) <= 2:
                        r, g, b = 0.38, 0.40, 0.43
                out[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]

    img = bpy.data.images.new("avental_couro_ferreiro_128", width=w, height=h, alpha=True)
    img.pixels[:] = out
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_leather_apron_mesh(under_bvh: BVHTree, under_coords: list[Vector]) -> tuple[bpy.types.Object, set[int]]:
    bm = bmesh.new()
    strap_faces: list[bmesh.types.BMFace] = []
    iron_faces: list[bmesh.types.BMFace] = []
    shadow_faces: list[bmesh.types.BMFace] = []
    hide_faces: list[bmesh.types.BMFace] = []
    interior_verts: set[bmesh.types.BMVert] = set()

    # 1) 360° Heavy Forged-Buckle Waist Belt (z = 1.040m .. 1.102m)
    n_belt = 32
    belt_z = [1.102, 1.071, 1.040]
    belt_rings = []
    for z_b in belt_z:
        band = [p for p in under_coords if abs(p.z - z_b) < 0.045 and abs(p.x) < 0.23]
        yc = 0.5 * (max((p.y for p in band), default=0.07) + min((p.y for p in band), default=-0.15))
        ring = []
        for k in range(n_belt):
            ang = k * 2.0 * math.pi / n_belt
            off = 0.0110 if abs(z_b - 1.071) < 0.01 else 0.0095
            ring.append(bm.verts.new(raycast_torso_point(under_bvh, ang, z_b, yc, off)))
        belt_rings.append(ring)

    for r_i in range(2):
        ra, rb = belt_rings[r_i], belt_rings[r_i + 1]
        for k in range(n_belt):
            kn = (k + 1) % n_belt
            strap_faces.append(bm.faces.new((ra[kn], ra[k], rb[k], rb[kn])))

    top_in = [Vector((-v.co.x, -(v.co.y + 0.045), -0.15)).normalized() for v in belt_rings[0]]
    rf, sf, rv, dv = extrude_closed_rim_inward(bm, belt_rings[0], top_in, rim_thick=0.0046, sink_depth=0.0045)
    strap_faces.extend(rf)
    shadow_faces.extend(sf)
    interior_verts.update(rv)
    interior_verts.update(dv)

    bot_in = [Vector((-v.co.x, -(v.co.y + 0.045), 0.15)).normalized() for v in list(reversed(belt_rings[-1]))]
    rf2, sf2, rv2, dv2 = extrude_closed_rim_inward(bm, list(reversed(belt_rings[-1])), bot_in, rim_thick=0.0046, sink_depth=0.0045)
    strap_faces.extend(rf2)
    shadow_faces.extend(sf2)
    interior_verts.update(rv2)
    interior_verts.update(dv2)

    # Forged Iron Rectangular Center Buckle
    bk_front_y = belt_rings[1][0].co.y + 0.0055
    bk_grid = [
        [Vector((x_b, bk_front_y - 0.0015 * abs(x_b) / 0.032, z_b)) for x_b in (-0.032, -0.011, 0.011, 0.032)]
        for z_b in (1.109, 1.086, 1.056, 1.033)
    ]
    bf_f, bf_r, bf_s, bf_int = add_extruded_patch_with_rim(bm, bk_grid, rim_thick=0.0048, sink_depth=0.0035)
    iron_faces.extend(bf_f)
    iron_faces.extend(bf_r)
    shadow_faces.extend(bf_s)
    interior_verts.update(bf_int)

    # 2) Broad Squared Heavy Rawhide Chest Bib (z = 1.094m .. 1.396m, wider heroic chest coverage!)
    bib_zs = [1.396, 1.335, 1.272, 1.212, 1.152, 1.094]
    bib_cols = 9
    bib_grid: list[list[Vector]] = []
    for z_l in bib_zs:
        t_up = (z_l - 1.094) / (1.396 - 1.094)
        half_w = 0.132 * (1.0 - t_up) + 0.106 * t_up
        band = [p for p in under_coords if abs(p.z - z_l) < 0.042 and abs(p.x) < 0.21]
        yc = 0.5 * (max((p.y for p in band), default=0.09) + min((p.y for p in band), default=-0.14))
        row = []
        for c_i in range(bib_cols):
            u_x = -1.0 + 2.0 * c_i / (bib_cols - 1)
            tx = u_x * half_w
            ang = math.atan2(tx, 0.155)
            pt = raycast_torso_point(under_bvh, ang, z_l, yc, offset=0.0090)
            pt.x = tx
            hit, _ = outermost_hit(under_bvh, Vector((tx, yc, z_l)), Vector((0.0, 1.0, 0.0)), 0.34)
            if hit is not None:
                pt.y = max(pt.y, hit.y + 0.0090)
            row.append(pt)
        peak_y = max(p.y for p in row)
        for c_i, p in enumerate(row):
            u_x = abs(-1.0 + 2.0 * c_i / (bib_cols - 1))
            p.y = max(p.y, peak_y - 0.009 * (u_x ** 2))
        bib_grid.append(row)

    bb_f, bb_r, bb_s, bb_int = add_extruded_patch_with_rim(bm, bib_grid, rim_thick=0.0052, sink_depth=0.0042)
    hide_faces.extend(bb_f)
    hide_faces.extend(bb_r)
    shadow_faces.extend(bb_s)
    interior_verts.update(bb_int)

    # Stitched Chest Tool Pocket on Bib
    pk_grid = []
    for z_p in (1.284, 1.222, 1.160):
        r_ref = bib_grid[2] if z_p > 1.25 else (bib_grid[3] if z_p > 1.19 else bib_grid[4])
        cy = max(p.y for p in r_ref) + 0.0052
        pk_grid.append([Vector((xp, cy - 0.002 * (abs(xp) / 0.062) ** 2, z_p)) for xp in (-0.062, -0.021, 0.021, 0.062)])
    pk_f, pk_r, pk_s, pk_int = add_extruded_patch_with_rim(bm, pk_grid, rim_thick=0.0044, sink_depth=0.0036)
    strap_faces.extend(pk_f)
    strap_faces.extend(pk_r)
    shadow_faces.extend(pk_s)
    interior_verts.update(pk_int)

    # 3) Over-Shoulder X-Back Leather Harness + Forged Iron Center Plate
    back_cross_pt = raycast_torso_point(under_bvh, math.pi, 1.268, -0.055, offset=0.0125)
    for side_idx, sx in enumerate((-1.0, 1.0)):
        strap_centers: list[tuple[Vector, Vector]] = []
        n_arc = 9
        for i_s in range(n_arc):
            u = i_s / (n_arc - 1)
            theta = 0.10 + u * (math.pi - 0.22)
            x_cur = sx * (0.088 + 0.020 * math.sin(u * math.pi))
            core = Vector((x_cur * 0.65, -0.055, 1.365))
            rdir = Vector((sx * 0.05 * math.sin(u * math.pi), math.cos(theta), math.sin(theta))).normalized()
            hit, hnrm = outermost_hit(under_bvh, core, rdir, 0.32)
            extra = 0.0095 + (0.0018 if side_idx == 1 and u > 0.75 else 0.0)
            pt = (hit + rdir * extra) if hit is not None else (core + rdir * (0.16 + extra))
            pt.x = x_cur
            n_out = hnrm.normalized() if (hnrm is not None and hnrm.length > 1e-4) else rdir
            strap_centers.append((pt, n_out))

        belt_end = raycast_torso_point(under_bvh, math.pi - sx * 0.58, 1.088, -0.055, offset=0.0105)
        mid_up = strap_centers[-1][0].lerp(back_cross_pt, 0.52)
        loc_m, nrm_m, _, _ = under_bvh.find_nearest(mid_up)
        if loc_m is not None and nrm_m is not None:
            mid_up = loc_m + nrm_m * 0.0105
        mid_dn = back_cross_pt.lerp(belt_end, 0.52)
        loc_d, nrm_d, _, _ = under_bvh.find_nearest(mid_dn)
        if loc_d is not None and nrm_d is not None:
            mid_dn = loc_d + nrm_d * 0.0105

        cross_p = back_cross_pt + Vector((0.0, -0.0020 * side_idx, 0.0))
        strap_centers.extend([
            (mid_up, Vector((0.0, -1.0, 0.15)).normalized()),
            (cross_p, Vector((0.0, -1.0, 0.0))),
            (mid_dn, Vector((0.0, -1.0, -0.05)).normalized()),
            (belt_end, Vector((-sx * 0.35, -0.94, 0.0)).normalized()),
        ])

        half_sw = 0.0165
        s_grid = []
        for idx_p, (cp, n_o) in enumerate(strap_centers):
            p_prev = strap_centers[max(0, idx_p - 1)][0]
            p_next = strap_centers[min(len(strap_centers) - 1, idx_p + 1)][0]
            tang = (p_next - p_prev).normalized()
            lat = tang.cross(n_o)
            if lat.length < 1e-4:
                lat = Vector((1.0, 0.0, 0.0))
            lat.normalize()
            if lat.x < 0.0:
                lat = -lat
            s_grid.append([cp - lat * half_sw, cp, cp + lat * half_sw])

        v_test_a = s_grid[1][0] - s_grid[0][0]
        v_test_b = s_grid[0][2] - s_grid[0][0]
        flip_s = v_test_a.cross(v_test_b).dot(strap_centers[0][1]) > 0.0
        sf_f, sf_r, sf_sh, sf_int = add_extruded_patch_with_rim(
            bm, s_grid, rim_thick=0.0042, sink_depth=0.0030, flip_winding=not flip_s
        )
        strap_faces.extend(sf_f)
        strap_faces.extend(sf_r)
        shadow_faces.extend(sf_sh)
        interior_verts.update(sf_int)

    med_y = back_cross_pt.y - 0.0055
    med_grid = [
        [Vector((xm, med_y + 0.0015 * (abs(xm) / 0.026), zm)) for xm in (-0.026, 0.0, 0.026)]
        for zm in (1.294, 1.268, 1.242)
    ]
    mf_f, mf_r, mf_s, mf_int = add_extruded_patch_with_rim(bm, med_grid, rim_thick=0.0045, sink_depth=0.0032, flip_winding=True)
    iron_faces.extend(mf_f)
    iron_faces.extend(mf_r)
    shadow_faces.extend(mf_s)
    interior_verts.update(mf_int)

    # 4) Split-Leg Blacksmith Lower Apron Shield (z = 1.055m down to z = 0.365m — smooth continuous cone over shirt hems!)
    sk_zs = [1.055, 0.968, 0.875, 0.775, 0.670, 0.568, 0.465, 0.365]
    hip_band = [p for p in under_coords if 0.84 <= p.z <= 1.02 and abs(p.x) < 0.25]
    hip_rx0 = max((abs(p.x) for p in hip_band), default=0.208) + 0.0110
    hip_yf0 = max((p.y for p in hip_band), default=0.082) + 0.0110
    hip_yb0 = min((p.y for p in hip_band), default=-0.155) - 0.0100

    for sx in (-1.0, 1.0):
        leg_grid = []
        n_lcols = 9
        for r_idx, z_s in enumerate(sk_zs):
            t_dn = (1.055 - z_s) / (1.055 - 0.365)
            slit_t = smoothstep(0.80, 0.365, z_s)
            ang_in = math.radians(1.0 + 6.8 * slit_t)
            ang_out = math.radians(108.0 - 14.0 * t_dn)

            band = [p for p in under_coords if abs(p.z - z_s) < 0.048 and abs(p.x) < 0.25]
            b_rx = max((abs(p.x) for p in band), default=0.195) + 0.0110
            b_yf = max((p.y for p in band), default=0.072) + 0.0110
            b_yb = min((p.y for p in band), default=-0.148) - 0.0100

            # Smooth blend from exact belt contour at r_idx=0 to full rawhide leg cone below z=0.95m (zero hip step!)
            cone_w = smoothstep(1.055, 0.90, z_s)
            rx_cone = max(b_rx, hip_rx0 + 0.026 * t_dn)
            yf_cone = max(b_yf, hip_yf0 + 0.020 * t_dn)
            yb_cone = min(b_yb, hip_yb0 - 0.012 * t_dn)
            yc = 0.5 * (yf_cone + yb_cone)
            ry_f = yf_cone - yc
            ry_b = yc - yb_cone

            row = []
            for c_i in range(n_lcols):
                u_c = c_i / (n_lcols - 1)
                ang_mag = (ang_out * (1.0 - u_c) + ang_in * u_c) if sx < 0 else (ang_in * (1.0 - u_c) + ang_out * u_c)
                ang = sx * ang_mag
                s_a, c_a = math.sin(ang), math.cos(ang)
                pt_cone = Vector((s_a * rx_cone, yc + c_a * (ry_f if c_a >= 0 else ry_b), z_s))
                pt_ray = raycast_torso_point(under_bvh, ang, z_s, yc, offset=0.0095)
                if cone_w < 0.99:
                    pt = pt_ray.lerp(pt_cone, cone_w)
                    # Never fall inside raycast hit!
                    hd = Vector((s_a, c_a, 0.0))
                    if (pt - pt_ray).dot(hd) < 0.0:
                        pt = pt_ray
                else:
                    pt = pt_cone
                row.append(pt)
            leg_grid.append(row)

        lf_f, lf_r, lf_s, lf_int = add_extruded_patch_with_rim(bm, leg_grid, rim_thick=0.0052, sink_depth=0.0045)
        hide_faces.extend(lf_f)
        hide_faces.extend(lf_r)
        shadow_faces.extend(lf_s)
        interior_verts.update(lf_int)

        if sx < 0:
            tp_grid = []
            for r_ref in (leg_grid[2], leg_grid[3], leg_grid[4]):
                sub = []
                for c_ref in (2, 3, 4, 5):
                    base_p = r_ref[c_ref]
                    out_d = Vector((base_p.x, base_p.y + 0.045, 0.0)).normalized()
                    sub.append(base_p + out_d * 0.0052)
                tp_grid.append(sub)
            tpf, tpr, tps, tp_int = add_extruded_patch_with_rim(bm, tp_grid, rim_thick=0.0044, sink_depth=0.0036)
            strap_faces.extend(tpf)
            strap_faces.extend(tpr)
            shadow_faces.extend(tps)
            interior_verts.update(tp_int)

    # 5) Right-Hip Hammer / Tongs Leather Loop
    hp_top = raycast_torso_point(under_bvh, math.radians(78.0), 1.055, -0.04, offset=0.014)
    hp_mid = hp_top + Vector((0.022, 0.004, -0.052))
    hp_bot = raycast_torso_point(under_bvh, math.radians(78.0), 0.955, -0.04, offset=0.017)
    loop_pts = [hp_top, hp_top.lerp(hp_mid, 0.55) + Vector((0.008, 0.0, 0.0)), hp_mid, hp_mid.lerp(hp_bot, 0.55) + Vector((0.008, 0.0, 0.0)), hp_bot]
    lp_grid = [[pt + Vector((0.0, -0.014, 0.0)), pt, pt + Vector((0.0, 0.014, 0.0))] for pt in loop_pts]
    hlf, hlr, hls, hl_int = add_extruded_patch_with_rim(bm, lp_grid, rim_thick=0.0038, sink_depth=0.0030, flip_winding=True)
    strap_faces.extend(hlf)
    strap_faces.extend(hlr)
    shadow_faces.extend(hls)
    interior_verts.update(hl_int)

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    enforce_mesh_clearance_over_bvh(bm, under_bvh, skip_verts=interior_verts, min_vert_gap=0.0065, min_face_gap=0.0052)

    uv_lay = bm.loops.layers.uv.new("UVMap")
    sh_set, ir_set, st_set = set(shadow_faces), set(iron_faces), set(strap_faces)
    for f in bm.faces:
        if f in sh_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.040)
        elif f in ir_set:
            c = f.calc_center_median()
            for lp in f.loops:
                p = lp.vert.co
                lp[uv_lay].uv = (clamp(0.50 + (p.x - c.x) * 6.0, 0.06, 0.94), clamp(0.15 + (p.z - c.z) * 1.1, 0.10, 0.20))
        elif f in st_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.48:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.03, 0.97), 0.245 if p.z < c.z else 0.352)
        else:
            c = f.calc_center_median()
            if c.z > 1.07:
                for lp in f.loops:
                    p = lp.vert.co
                    lp[uv_lay].uv = (clamp(0.50 + p.x / 0.28, 0.03, 0.97), clamp(0.41 + (p.z - 1.094) / (1.40 - 1.094) * 0.56, 0.40, 0.97))
            else:
                sx_f = -1.0 if c.x < 0 else 1.0
                for lp in f.loops:
                    p = lp.vert.co
                    ang_m = abs(math.atan2(p.x, p.y + 0.045))
                    u_v = clamp(0.04 + (ang_m / math.radians(110.0)) * 0.92, 0.03, 0.97)
                    if sx_f < 0:
                        u_v = 1.0 - u_v
                    lp[uv_lay].uv = (u_v, clamp(0.40 + (p.z - 0.365) / (1.055 - 0.365) * 0.57, 0.39, 0.97))

    final_int = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("AventalCouroFerreiro")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("AventalCouroFerreiro", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_int


# =============================================================================
# 2. AVENTAL DE PANO (TAVERNA / COZINHEIRA / CURANDEIRA) — TEXTURE + MESH
# =============================================================================
def generate_cloth_apron_texture(cloth_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    src = bpy.data.images.load(str(cloth_png), check_existing=True)
    w, h = src.size
    sp = list(src.pixels[:])
    out = [0.0] * (w * h * 4)

    lum = [[0.0] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            i = (y * w + x) * 4
            lum[y][x] = 0.299 * sp[i] + 0.587 * sp[i + 1] + 0.114 * sp[i + 2]

    sig = [[0.0] * w for _ in range(h)]
    vals = []
    for y in range(h):
        for x in range(w):
            s = sum(lum[(y + dy) % h][(x + dx) % w] for dy in range(-3, 4) for dx in range(-3, 4)) / 49.0
            v_s = (lum[y][x] - s) * 0.95 + (s - 0.18) * 0.25
            sig[y][x] = v_s
            vals.append(v_s)

    vs = sorted(vals)
    v_lo, v_hi = vs[int(0.03 * len(vs))], vs[int(0.97 * len(vs))]
    v_span = max(v_hi - v_lo, 1e-4)

    c_dark = (0.58, 0.52, 0.43)
    c_mid = (0.76, 0.70, 0.59)
    c_hi = (0.89, 0.84, 0.74)

    s_dark = (0.21, 0.28, 0.20)
    s_hi = (0.42, 0.52, 0.38)

    b_rust = (0.56, 0.24, 0.19)
    b_cream = (0.88, 0.83, 0.73)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((sig[y][x] - v_lo) / v_span, 0.0, 1.0)

            if v < 0.085:
                f = 0.82 + 0.34 * tn
                out[idx : idx + 4] = [0.16 * f, 0.14 * f, 0.12 * f, 1.0]
            elif v < 0.215:
                dy = y - 19
                d_dist = abs((x % 8) - 4) + abs(dy)
                if y in (13, 25):
                    r, g, b = s_dark[0] + 0.12 * tn, s_dark[1] + 0.14 * tn, s_dark[2] + 0.10 * tn
                elif d_dist <= 2:
                    r, g, b = b_rust[0] * (0.88 + 0.28 * tn), b_rust[1] * (0.88 + 0.28 * tn), b_rust[2] * (0.88 + 0.28 * tn)
                elif d_dist <= 4:
                    r, g, b = b_cream[0] * (0.90 + 0.16 * tn), b_cream[1] * (0.90 + 0.16 * tn), b_cream[2] * (0.90 + 0.16 * tn)
                else:
                    r = s_dark[0] + (s_hi[0] - s_dark[0]) * tn
                    g = s_dark[1] + (s_hi[1] - s_dark[1]) * tn
                    b = s_dark[2] + (s_hi[2] - s_dark[2]) * tn
                out[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            elif v < 0.375:
                rib = 0.92 + 0.14 * math.cos(y * math.pi * 0.5)
                tw = clamp(tn * rib, 0.0, 1.0)
                r = s_dark[0] + (s_hi[0] - s_dark[0]) * tw
                g = s_dark[1] + (s_hi[1] - s_dark[1]) * tw
                b = s_dark[2] + (s_hi[2] - s_dark[2]) * tw
                if y in (30, 45) and ((x // 2) % 2 == 0):
                    r, g, b = r + 0.18, g + 0.18, b + 0.15
                out[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = c_dark[0] + (c_mid[0] - c_dark[0]) * k
                    g = c_dark[1] + (c_mid[1] - c_dark[1]) * k
                    b = c_dark[2] + (c_mid[2] - c_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = c_mid[0] + (c_hi[0] - c_mid[0]) * k
                    g = c_mid[1] + (c_hi[1] - c_mid[1]) * k
                    b = c_mid[2] + (c_hi[2] - c_mid[2]) * k
                if (x % 8) == 4:
                    r, g, b = r * 0.86, g * 0.89, b * 0.84
                hem_sh = 1.0 - 0.08 * smoothstep(0.48, 0.38, v)
                out[idx : idx + 4] = [clamp(r * hem_sh), clamp(g * hem_sh), clamp(b * hem_sh), 1.0]

    img = bpy.data.images.new("avental_pano_taverna_128", width=w, height=h, alpha=True)
    img.pixels[:] = out
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_cloth_apron_mesh(under_bvh: BVHTree, under_coords: list[Vector]) -> tuple[bpy.types.Object, set[int]]:
    bm = bmesh.new()
    sash_faces: list[bmesh.types.BMFace] = []
    trim_faces: list[bmesh.types.BMFace] = []
    shadow_faces: list[bmesh.types.BMFace] = []
    interior_verts: set[bmesh.types.BMVert] = set()

    # 1) Pleated Sage-Olive Cloth Waist Sash (z = 1.044m .. 1.098m) + 3D Back Bow & Tails!
    n_sash = 32
    sash_zs = [1.098, 1.071, 1.044]
    sash_rings = []
    for z_s in sash_zs:
        band = [p for p in under_coords if abs(p.z - z_s) < 0.045 and abs(p.x) < 0.24]
        yc = 0.5 * (max((p.y for p in band), default=0.075) + min((p.y for p in band), default=-0.155))
        ring = []
        for k in range(n_sash):
            ang = k * 2.0 * math.pi / n_sash
            off = 0.0105 if abs(z_s - 1.071) < 0.01 else 0.0090
            ring.append(bm.verts.new(raycast_torso_point(under_bvh, ang, z_s, yc, off)))
        sash_rings.append(ring)

    for r_i in range(2):
        ra, rb = sash_rings[r_i], sash_rings[r_i + 1]
        for k in range(n_sash):
            kn = (k + 1) % n_sash
            sash_faces.append(bm.faces.new((ra[kn], ra[k], rb[k], rb[kn])))

    t_in = [Vector((-v.co.x, -(v.co.y + 0.045), -0.15)).normalized() for v in sash_rings[0]]
    rf, sf, rv, dv = extrude_closed_rim_inward(bm, sash_rings[0], t_in, rim_thick=0.0040, sink_depth=0.0042)
    sash_faces.extend(rf)
    shadow_faces.extend(sf)
    interior_verts.update(rv)
    interior_verts.update(dv)

    b_in = [Vector((-v.co.x, -(v.co.y + 0.045), 0.15)).normalized() for v in list(reversed(sash_rings[-1]))]
    rf2, sf2, rv2, dv2 = extrude_closed_rim_inward(bm, list(reversed(sash_rings[-1])), b_in, rim_thick=0.0040, sink_depth=0.0042)
    sash_faces.extend(rf2)
    shadow_faces.extend(sf2)
    interior_verts.update(rv2)
    interior_verts.update(dv2)

    # 3D Cloth Back Knot + 2 Bow Loops + 2 Flowing Sash Tails (generous clearance behind back!)
    knot_pt = sash_rings[1][n_sash // 2].co + Vector((0.0, -0.012, 0.0))
    kn_grid = [
        [knot_pt + Vector((xk, 0.003 * abs(xk) / 0.020, zk)) for xk in (-0.020, 0.0, 0.020)]
        for zk in (0.022, 0.0, -0.022)
    ]
    kf, kr, ks, k_int = add_extruded_patch_with_rim(bm, kn_grid, rim_thick=0.0052, sink_depth=0.0035, flip_winding=True)
    sash_faces.extend(kf)
    sash_faces.extend(kr)
    shadow_faces.extend(ks)
    interior_verts.update(k_int)

    for sx in (-1.0, 1.0):
        wing_grid = []
        for zw in (0.028, 0.0, -0.024):
            wing_grid.append([
                knot_pt + Vector((sx * 0.014, -0.002, zw * 0.55)),
                knot_pt + Vector((sx * 0.048, -0.016, zw * 0.95 + 0.006)),
                knot_pt + Vector((sx * 0.078, -0.014, zw * 0.70 + 0.004)),
            ])
        wf, wr, ws, w_int = add_extruded_patch_with_rim(
            bm, wing_grid, rim_thick=0.0040, sink_depth=0.0028, flip_winding=(sx > 0)
        )
        sash_faces.extend(wf)
        sash_faces.extend(wr)
        shadow_faces.extend(ws)
        interior_verts.update(w_int)

        tail_grid = []
        for i_t, z_t in enumerate((1.055, 0.985, 0.910, 0.835, 0.755)):
            u_t = i_t / 4.0
            xc = sx * (0.020 + 0.042 * u_t)
            ang_t = math.pi - math.atan2(xc, 0.16)
            base_t = raycast_torso_point(under_bvh, ang_t, z_t, -0.055, offset=0.021 + 0.008 * u_t)
            hw = 0.015 + 0.007 * u_t
            tail_grid.append([
                Vector((xc - hw, base_t.y, z_t)),
                Vector((xc, base_t.y - 0.003, z_t - 0.004 * u_t)),
                Vector((xc + hw, base_t.y, z_t)),
            ])
        tf_f, tf_r, tf_s, tf_int = add_extruded_patch_with_rim(bm, tail_grid, rim_thick=0.0038, sink_depth=0.0028, flip_winding=True)
        sash_faces.extend(tf_f)
        sash_faces.extend(tf_r)
        shadow_faces.extend(tf_s)
        interior_verts.update(tf_int)

    # 2) Soft Curved Sweetheart Pinafore Bib (z = 1.092m .. 1.344m, fuller cozy proportions!)
    pbib_zs = [1.344, 1.292, 1.240, 1.190, 1.140, 1.092]
    pbib_cols = 9
    pbib_grid: list[list[Vector]] = []
    for r_i, z_b in enumerate(pbib_zs):
        t_up = (z_b - 1.092) / (1.344 - 1.092)
        half_w = 0.112 * (1.0 - t_up) + 0.092 * t_up
        band = [p for p in under_coords if abs(p.z - z_b) < 0.042 and abs(p.x) < 0.21]
        yc = 0.5 * (max((p.y for p in band), default=0.09) + min((p.y for p in band), default=-0.14))
        row = []
        for c_i in range(pbib_cols):
            u_x = -1.0 + 2.0 * c_i / (pbib_cols - 1)
            tx = u_x * half_w
            dip = 0.016 * (1.0 - u_x ** 2) * (1.0 - r_i / 3.0) if r_i < 3 else 0.0
            zz = z_b - dip
            ang = math.atan2(tx, 0.155)
            pt = raycast_torso_point(under_bvh, ang, zz, yc, offset=0.0090)
            pt.x = tx
            hit, _ = outermost_hit(under_bvh, Vector((tx, yc, zz)), Vector((0.0, 1.0, 0.0)), 0.34)
            if hit is not None:
                pt.y = max(pt.y, hit.y + 0.0090)
            row.append(pt)
        peak_y = max(p.y for p in row)
        for c_i, p in enumerate(row):
            u_x = abs(-1.0 + 2.0 * c_i / (pbib_cols - 1))
            p.y = max(p.y, peak_y - 0.008 * (u_x ** 2))
        pbib_grid.append(row)

    pb_f, pb_r, pb_s, pb_int = add_extruded_patch_with_rim(bm, pbib_grid, rim_thick=0.0040, sink_depth=0.0036)
    for idx_f, fq in enumerate(pb_f):
        if idx_f < (pbib_cols - 1):
            trim_faces.append(fq)
    shadow_faces.extend(pb_s)
    interior_verts.update(pb_int)

    # 3) Clean Collar-Hugging Halter Neck Loop Strap (curves closely around neck base |x|<=0.084m, NEVER wide over deltoids!)
    # Trace left bib top corner (-0.082, z=1.340) -> left inner trapezius (-0.078, y=-0.045, z=1.522) -> nape (0, y=-0.122, z=1.505) -> right!
    n_halter = 17
    halter_centers: list[tuple[Vector, Vector]] = []
    for i_h in range(n_halter):
        u_h = i_h / (n_halter - 1)  # 0 = front-left bib corner, 0.5 = center-back nape, 1 = front-right bib corner
        # Angle around neck center (0, -0.052): from -32° (front-left) around back (-180°) to +32° (front-right)
        ang_deg = -32.0 - u_h * (360.0 - 64.0)
        ang_r = math.radians(ang_deg)
        sa, ca = math.sin(ang_r), math.cos(ang_r)

        # Smooth anatomical neck-ring ellipse: rx = 0.082m (snug outside neck/collar!), ry_front = 0.14m, ry_back = 0.076m
        rx_h = 0.084
        y_cand = -0.052 + ca * (0.145 if ca >= 0 else 0.076)
        x_cand = sa * rx_h

        # Height: rises from bib corner (1.340) over inner collarbone/trapezius peak (1.522 at u=0.26/0.74) and dips gently at nape (1.506 at u=0.50)
        if u_h <= 0.5:
            t_s = smoothstep(0.0, 0.30, u_h)
            nape_dip = 0.014 * smoothstep(0.30, 0.50, u_h)
        else:
            t_s = smoothstep(1.0, 0.70, u_h)
            nape_dip = 0.014 * smoothstep(0.70, 0.50, u_h)
        z_cand = 1.340 + 0.184 * t_s - nape_dip

        pt = Vector((x_cand, y_cand, z_cand))
        # Ensure clean clearance above upper chest / trapezius / shirt collar via downward/inward raycast toward neck-chest core (0, -0.052, 1.38)
        core = Vector((x_cand * 0.35, -0.052, 1.385))
        rdir = (pt - core).normalized()
        hit, hnrm = outermost_hit(under_bvh, core, rdir, 0.30)
        if hit is not None:
            min_len = (hit - core).length + 0.0090
            if (pt - core).length < min_len:
                pt = core + rdir * min_len
        n_o = hnrm.normalized() if (hnrm is not None and hnrm.length > 1e-4) else rdir
        halter_centers.append((pt, n_o))

    h_grid = []
    half_hw = 0.0100
    for idx_h, (cp, n_o) in enumerate(halter_centers):
        p_prev = halter_centers[max(0, idx_h - 1)][0]
        p_next = halter_centers[min(n_halter - 1, idx_h + 1)][0]
        tang = (p_next - p_prev).normalized()
        lat = tang.cross(n_o)
        if lat.length < 1e-4:
            lat = Vector((0.0, 0.0, 1.0))
        lat.normalize()
        h_grid.append([cp - lat * half_hw, cp, cp + lat * half_hw])

    va_h = h_grid[1][0] - h_grid[0][0]
    vb_h = h_grid[0][2] - h_grid[0][0]
    flip_h = va_h.cross(vb_h).dot(halter_centers[0][1]) > 0.0
    hf_f, hf_r, hf_s, hf_int = add_extruded_patch_with_rim(
        bm, h_grid, rim_thick=0.0034, sink_depth=0.0026, flip_winding=not flip_h
    )
    sash_faces.extend(hf_f)
    sash_faces.extend(hf_r)
    shadow_faces.extend(hf_s)
    interior_verts.update(hf_int)

    # 4) Gathered Rounded-Corner Front Apron Skirt (z = 1.052m down to z = 0.340m)
    ap_zs = [1.052, 0.964, 0.868, 0.768, 0.662, 0.556, 0.446, 0.378, 0.340]
    ap_cols = 17
    hip_b = [p for p in under_coords if 0.84 <= p.z <= 1.00 and abs(p.x) < 0.25]
    ap_rx0 = max((abs(p.x) for p in hip_b), default=0.208) + 0.0105
    ap_yf0 = max((p.y for p in hip_b), default=0.084) + 0.0105
    ap_yb0 = min((p.y for p in hip_b), default=-0.155) - 0.0095

    ap_grid: list[list[Vector]] = []
    for r_idx, z_a in enumerate(ap_zs):
        t_dn = (1.052 - z_a) / (1.052 - 0.340)
        corner_round = smoothstep(0.52, 0.340, z_a)
        max_ang = math.radians(74.0 - 8.0 * t_dn - 12.0 * (corner_round ** 1.4))
        pleat_amp = 0.0010 + 0.0044 * t_dn

        band = [p for p in under_coords if abs(p.z - z_a) < 0.048 and abs(p.x) < 0.31]
        b_rx = max((abs(p.x) for p in band), default=0.212) + 0.0105
        b_yf = max((p.y for p in band), default=0.082) + 0.0105
        b_yb = min((p.y for p in band), default=-0.150) - 0.0095

        t_cone = clamp((0.935 - z_a) / (0.935 - 0.125), 0.0, 1.0)
        t_bell = 0.78 * t_cone + 0.22 * (t_cone ** 1.35)
        rx_cone = max(b_rx, ap_rx0 + 0.086 * t_bell)
        yf_cone = max(b_yf, ap_yf0 + 0.072 * t_bell)
        yb_cone = min(b_yb, ap_yb0 - 0.056 * t_bell)
        yc = 0.5 * (yf_cone + yb_cone)
        ry_f = yf_cone - yc
        cone_w = smoothstep(1.052, 0.90, z_a)

        row = []
        for c_i in range(ap_cols):
            u_x = -1.0 + 2.0 * c_i / (ap_cols - 1)
            ang = u_x * max_ang
            pleat = math.cos(u_x * math.pi * 4.0) * pleat_amp * (1.0 - 0.45 * (u_x ** 4))
            corner_lift = 0.032 * (abs(u_x) ** 2.6) * smoothstep(0.60, 0.340, z_a)
            zz = z_a + corner_lift
            sa, ca = math.sin(ang), math.cos(ang)
            pt_cone = Vector((sa * (rx_cone + pleat), yc + ca * (ry_f + pleat), zz))
            pt_ray = raycast_torso_point(under_bvh, ang, zz, yc, offset=0.0095 + max(0.0, pleat))
            if cone_w < 0.99:
                pt = pt_ray.lerp(pt_cone, cone_w)
                hd = Vector((sa, ca, 0.0))
                if (pt - pt_ray).dot(hd) < 0.0:
                    pt = pt_ray
            else:
                pt = pt_cone
            row.append(pt)
        ap_grid.append(row)

    ap_f, ap_r, ap_s, ap_int = add_extruded_patch_with_rim(bm, ap_grid, rim_thick=0.0044, sink_depth=0.0042)
    n_c_q = ap_cols - 1
    for idx_f, fq in enumerate(ap_f):
        if (idx_f // n_c_q) == len(ap_zs) - 2:
            trim_faces.append(fq)
    trim_faces.extend(ap_r)
    shadow_faces.extend(ap_s)
    interior_verts.update(ap_int)

    # 5) Divided Double Herb / Tavern Pocket (z = 0.775m .. 0.915m)
    dpk_grid = []
    for r_src in (ap_grid[1], ap_grid[2], ap_grid[3]):
        prow = []
        for c_src in (4, 6, 8, 10, 12):
            bp = r_src[c_src]
            out_n = Vector((bp.x, bp.y + 0.045, 0.0)).normalized()
            lift = 0.0026 if c_src == 8 else 0.0065
            prow.append(bp + out_n * lift + Vector((0.0, 0.0, -0.028)))
        dpk_grid.append(prow)
    dp_f, dp_r, dp_s, dp_int = add_extruded_patch_with_rim(bm, dpk_grid, rim_thick=0.0040, sink_depth=0.0034)
    for idx_f, fq in enumerate(dp_f):
        if idx_f < 4:
            sash_faces.append(fq)
    shadow_faces.extend(dp_s)
    interior_verts.update(dp_int)

    # 6) Tucked Front-Left Kitchen/Healer Hand-Towel (anchored directly over ap_grid cols 1..3, rows 0..3 -> 100% snug & vertical!)
    tw_grid = []
    for r_i, r_src in enumerate((ap_grid[0], ap_grid[1], ap_grid[2], ap_grid[3])):
        u_tw = r_i / 3.0
        sub = []
        for j_tw, c_src in enumerate((1, 2, 3)):
            bp = r_src[c_src]
            out_n = Vector((bp.x, bp.y + 0.045, 0.0)).normalized()
            fold = 0.0045 * (1.0 if j_tw == 1 else 0.0) * (0.35 + 0.65 * u_tw)
            sub.append(bp + out_n * (0.0072 + 0.0025 * u_tw + fold) + Vector((0.0, 0.0, 0.006 - 0.012 * u_tw)))
        tw_grid.append(sub)
    tw_f, tw_r, tw_s, tw_int = add_extruded_patch_with_rim(bm, tw_grid, rim_thick=0.0036, sink_depth=0.0030)
    for idx_f, fq in enumerate(tw_f):
        if idx_f >= len(tw_f) - 2:
            trim_faces.append(fq)
    shadow_faces.extend(tw_s)
    interior_verts.update(tw_int)

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    enforce_mesh_clearance_over_bvh(bm, under_bvh, skip_verts=interior_verts, min_vert_gap=0.0065, min_face_gap=0.0052)

    uv_lay = bm.loops.layers.uv.new("UVMap")
    sh_set, sa_set, tr_set = set(shadow_faces), set(sash_faces), set(trim_faces)
    for f in bm.faces:
        if f in sh_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.040)
        elif f in sa_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.48:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.03, 0.97), 0.245 if p.z < c.z else 0.352)
        elif f in tr_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 1.10, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.48:
                u_raws = [(lp, u + 1.10 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.03, 0.97), 0.105 if p.z < c.z else 0.198)
        else:
            c = f.calc_center_median()
            if c.z > 1.07:
                for lp in f.loops:
                    p = lp.vert.co
                    lp[uv_lay].uv = (clamp(0.50 + p.x / 0.25, 0.04, 0.96), clamp(0.42 + (p.z - 1.09) / (1.35 - 1.09) * 0.54, 0.40, 0.97))
            else:
                for lp in f.loops:
                    p = lp.vert.co
                    ang = math.atan2(p.x, p.y + 0.045)
                    lp[uv_lay].uv = (
                        clamp(0.50 + ang / math.radians(165.0) * 0.90, 0.03, 0.97),
                        clamp(0.40 + (p.z - 0.34) / (1.05 - 0.34) * 0.57, 0.39, 0.97),
                    )

    final_int = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("AventalPanoTaverna")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("AventalPanoTaverna", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_int


# =============================================================================
# PREVIEW RENDERING OVER FULL EQUIPPED OUTFITS
# =============================================================================
def render_apron_previews(
    apron_obj: bpy.types.Object,
    body_obj: bpy.types.Object,
    arm_obj: bpy.types.Object,
    outfit_map: dict[str, bpy.types.Object],
    out_dir: pathlib.Path,
    angles_png: str,
    types_png: str,
    mode: str,
):
    scene = bpy.context.scene
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"
    if hasattr(scene, "eevee") and hasattr(scene.eevee, "taa_render_samples"):
        scene.eevee.taa_render_samples = 12

    world = bpy.data.worlds.new(f"ApronWorld_{mode}")
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.052, 0.058, 0.072, 1.0)

    lights = []
    for lname, loc, energy, col in [
        ("ApKey", (-2.2, 3.8, 2.8), 3.4, (1.0, 0.97, 0.92)),
        ("ApFill", (2.8, 2.8, 1.8), 1.85, (0.82, 0.88, 1.0)),
        ("ApRim", (0.2, -3.8, 2.5), 2.5, (0.95, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy, ldata.color = energy, col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 0.95)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("ApCam")
    cam = bpy.data.objects.new("ApCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    fem_mat = None
    fem_img = bpy.data.images.get("Chartest-Female-v01-128.png")
    if fem_img is not None:
        fem_mat = bpy.data.materials.new(f"ApFemSkin_{mode}")
        fem_mat.use_nodes = True
        ns, ls = fem_mat.node_tree.nodes, fem_mat.node_tree.links
        ns.clear()
        mo, bs, ti = ns.new("ShaderNodeOutputMaterial"), ns.new("ShaderNodeBsdfPrincipled"), ns.new("ShaderNodeTexImage")
        bs.inputs["Roughness"].default_value = 0.9
        ti.image, ti.interpolation = fem_img, "Closest"
        ls.new(ti.outputs["Color"], bs.inputs["Base Color"])
        ls.new(bs.outputs["BSDF"], mo.inputs["Surface"])

    deps = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = arm_obj.hide_render = apron_obj.hide_render = True
    for o in outfit_map.values():
        o.hide_render = True
        if "Armature" in o.modifiers:
            o.modifiers["Armature"].object = arm_obj

    def sync_cloth_obj(co_obj, is_female, breasts, glutes, height_m):
        if co_obj.data.shape_keys:
            for kb in co_obj.data.shape_keys.key_blocks[1:]:
                kb.value = body_obj.data.shape_keys.key_blocks[kb.name].value if kb.name in body_obj.data.shape_keys.key_blocks else 0.0
            co_obj.data.shape_keys.key_blocks["Feminine_Body"].value = 1.0 if is_female else 0.0
            co_obj.data.shape_keys.key_blocks["Female_Breasts"].value = breasts if is_female else 0.0
            co_obj.data.shape_keys.key_blocks["Glutes_Mass"].value = glutes
            co_obj.data.shape_keys.key_blocks["_Height_Short_Comp"].value = clamp((1.82 - height_m) / (1.82 - 0.50), 0.0, 1.0) if height_m < 1.82 else 0.0
            co_obj.data.shape_keys.key_blocks["_Height_Tall_Comp"].value = clamp((height_m - 1.82) / (2.50 - 1.82), 0.0, 1.0) if height_m > 1.82 else 0.0
        co_obj.update_tag()

    def spawn(x_p, yaw, clothes_keys=(), include_body=True, is_female=False, breasts=0.0, glutes=0.0, height_m=1.82, sk=None):
        for kb in body_obj.data.shape_keys.key_blocks[1:]:
            kb.value = 0.0
        arm_obj["height_m"] = height_m
        arm_obj["is_female"] = is_female
        arm_obj["breasts_size"] = breasts
        arm_obj["glutes_size"] = glutes
        if sk:
            for k, val in sk.items():
                body_obj.data.shape_keys.key_blocks[k].value = val
        set_relaxed_preview_pose(arm_obj, relax_deg=17.0)
        for ck in clothes_keys:
            if ck in outfit_map:
                sync_cloth_obj(outfit_map[ck], is_female, breasts, glutes, height_m)
        arm_obj.update_tag()
        body_obj.update_tag()
        apron_obj.update_tag()
        deps.update()

        objs = []
        rot = (0.0, 0.0, math.radians(yaw))
        if include_body:
            ev_b = body_obj.evaluated_get(deps)
            mb = bpy.data.meshes.new_from_object(ev_b, preserve_all_data_layers=True, depsgraph=deps)
            ob = bpy.data.objects.new("SnapBody", mb)
            if is_female and fem_mat is not None:
                ob.data.materials.clear()
                ob.data.materials.append(fem_mat)
            scene.collection.objects.link(ob)
            ob.location, ob.rotation_euler = (x_p, 0.0, 0.0), rot
            objs.append(ob)

        for ck in clothes_keys:
            if ck in outfit_map:
                ev_c = outfit_map[ck].evaluated_get(deps)
                mc = bpy.data.meshes.new_from_object(ev_c, preserve_all_data_layers=True, depsgraph=deps)
                oc = bpy.data.objects.new(f"Snap_{ck}", mc)
                scene.collection.objects.link(oc)
                oc.location, oc.rotation_euler = (x_p, 0.0, 0.0), rot
                objs.append(oc)

        ev_a = apron_obj.evaluated_get(deps)
        ma = bpy.data.meshes.new_from_object(ev_a, preserve_all_data_layers=True, depsgraph=deps)
        oa = bpy.data.objects.new("SnapApron", ma)
        scene.collection.objects.link(oa)
        oa.location, oa.rotation_euler = (x_p, 0.0, 0.0), rot
        objs.append(oa)
        return objs

    def clean(objs):
        for o in objs:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            if m and m.users == 0:
                bpy.data.meshes.remove(m)

    if mode == "leather":
        snaps = []
        base_sk = {"Body_Muscle": 0.70, "Shoulders_Width": 0.35, "Arms_Upper": 0.40}
        for x_p, yaw in [(1.62, 0.0), (0.54, 34.0), (-0.52, -90.0), (-1.56, -152.0)]:
            snaps.extend(spawn(x_p, yaw, clothes_keys=("short_shirt", "pants"), is_female=False, height_m=1.84, sk=base_sk))
        scene.render.resolution_x, scene.render.resolution_y = 1760, 820
        cam.location = (0.04, 4.2, 0.94)
        cam.rotation_euler = (Vector((0.04, 0.0, 0.92)) - cam.location).to_track_quat("-Z", "Y").to_euler()
        cam_data.ortho_scale = 4.45
        scene.render.filepath = str(out_dir / angles_png)
        bpy.ops.render.render(write_still=True)
        clean(snaps)

        snaps = []
        snaps.extend(spawn(2.90, 26.0, clothes_keys=("linen_shirt", "pants"), is_female=False, height_m=1.86, sk={"Body_Muscle": 0.95, "Shoulders_Width": 0.50}))
        snaps.extend(spawn(1.78, 28.0, clothes_keys=("blouse", "pants"), is_female=True, breasts=1.20, glutes=1.00, height_m=1.74))
        snaps.extend(spawn(0.68, 32.0, clothes_keys=("blouse", "knee_pants"), is_female=True, breasts=1.95, glutes=1.85, height_m=1.74))
        snaps.extend(spawn(-0.44, 26.0, clothes_keys=("short_shirt", "knee_pants"), is_female=False, glutes=0.50, height_m=1.78, sk={"Body_Heavy_Fat": 0.95, "Abdomen_Belly": 0.60}))
        snaps.extend(spawn(-1.58, -145.0, clothes_keys=("pants",), is_female=False, height_m=1.82))
        iso = spawn(-2.74, 28.0, clothes_keys=(), include_body=False, height_m=1.82)
        for o in iso:
            o.scale = (1.18, 1.18, 1.18)
            o.location = (-2.74, 0.0, -0.05)
        snaps.extend(iso)
    else:
        snaps = []
        for x_p, yaw in [(1.62, 0.0), (0.54, 34.0), (-0.52, -90.0), (-1.56, -152.0)]:
            snaps.extend(spawn(x_p, yaw, clothes_keys=("blouse", "skirt"), is_female=True, breasts=1.10, glutes=0.95, height_m=1.74))
        scene.render.resolution_x, scene.render.resolution_y = 1760, 820
        cam.location = (0.04, 4.2, 0.90)
        cam.rotation_euler = (Vector((0.04, 0.0, 0.88)) - cam.location).to_track_quat("-Z", "Y").to_euler()
        cam_data.ortho_scale = 4.45
        scene.render.filepath = str(out_dir / angles_png)
        bpy.ops.render.render(write_still=True)
        clean(snaps)

        snaps = []
        snaps.extend(spawn(2.90, 26.0, clothes_keys=("blouse", "skirt"), is_female=True, breasts=1.15, glutes=0.95, height_m=1.74))
        snaps.extend(spawn(1.78, -144.0, clothes_keys=("blouse", "skirt"), is_female=True, breasts=1.95, glutes=1.95, height_m=1.74))
        snaps.extend(spawn(0.68, 26.0, clothes_keys=("linen_shirt", "skirt"), is_female=True, breasts=1.45, glutes=1.35, height_m=1.71, sk={"Body_Heavy_Fat": 0.85, "Hips_Width": 0.40}))
        snaps.extend(spawn(-0.44, 26.0, clothes_keys=("linen_shirt", "knee_pants"), is_female=True, breasts=0.50, glutes=0.40, height_m=1.61, sk={"Body_Gaunt_Lean": 0.60}))
        snaps.extend(spawn(-1.58, 26.0, clothes_keys=("short_shirt", "pants"), is_female=False, height_m=1.84, sk={"Body_Heavy_Fat": 0.45, "Abdomen_Belly": 0.40}))
        iso = spawn(-2.74, 28.0, clothes_keys=(), include_body=False, height_m=1.82)
        for o in iso:
            o.scale = (1.18, 1.18, 1.18)
            o.location = (-2.74, 0.0, -0.05)
        snaps.extend(iso)

    scene.render.resolution_x, scene.render.resolution_y = 1920, 820
    cam.location = (0.04, 5.5, 0.94)
    cam.rotation_euler = (Vector((0.04, 0.0, 0.92)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 7.20
    scene.render.filepath = str(out_dir / types_png)
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    for lobj in lights:
        ld = lobj.data
        bpy.data.objects.remove(lobj, do_unlink=True)
        bpy.data.lights.remove(ld)
    bpy.data.objects.remove(cam, do_unlink=True)
    bpy.data.cameras.remove(cam_data)
    set_relaxed_preview_pose(arm_obj, relax_deg=0.0)
    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"], arm_obj["is_female"], arm_obj["breasts_size"], arm_obj["glutes_size"] = 1.82, False, 0.0, 0.0
    body_obj.hide_render = arm_obj.hide_render = apron_obj.hide_render = False


def cleanup_imported_objects(objs: list[bpy.types.Object]):
    for o in objs:
        if o.name in bpy.data.objects:
            bpy.data.objects.remove(o, do_unlink=True)


def run_leather_apron():
    OUT_LEATHER.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, _, _ = open_clean_chartest()

    ref_meshes, all_imp = import_reference_garments([
        "CamisaLinhoClara.glb",
        "CamisaMangaCurtaBranca.glb",
        "BlusaFeminina.glb",
        "CalcaLinho.glb",
        "CalcaCurtaJoelho.glb",
    ])
    outfit_map = {}
    for m in ref_meshes:
        if "CamisaLinhoClara" in m.name:
            outfit_map["linen_shirt"] = m
        elif "CamisaMangaCurtaBranca" in m.name:
            outfit_map["short_shirt"] = m
        elif "BlusaFeminina" in m.name:
            outfit_map["blouse"] = m
        elif "CalcaLinho" in m.name:
            outfit_map["pants"] = m
        elif "CalcaCurtaJoelho" in m.name:
            outfit_map["knee_pants"] = m

    under_garments = [outfit_map[k] for k in ("linen_shirt", "short_shirt", "blouse", "pants") if k in outfit_map]
    under_bvh, under_coords = build_composite_under_bvh(body_obj, under_garments, exclude_arms_x=0.205, exclude_head_z=1.600)

    tex_img = generate_leather_apron_texture(COURO_DARK_PNG, COURO_LIGHT_PNG, OUT_LEATHER / "avental_couro_ferreiro_128.png")
    apron_obj, interior_idx = build_leather_apron_mesh(under_bvh, under_coords)
    apron_obj.parent = arm_obj
    export_uv_overlay(apron_obj, OUT_LEATHER / "avental_couro_ferreiro_uv.png")

    autofit_garment_to_chartest(apron_obj, body_obj, arm_obj, interior_idx)
    sanitize_apron_weights(apron_obj)
    lockstep_over_clothes_morphs(apron_obj, body_obj, under_garments, interior_idx)

    verify_garment_and_export_glb(apron_obj, body_obj, arm_obj, tex_img, OUT_LEATHER / "AventalCouroFerreiro.glb", interior_idx)
    audit_over_outfit_clearance(apron_obj, body_obj, arm_obj, under_garments, interior_idx, "LeatherApron")

    render_apron_previews(
        apron_obj,
        body_obj,
        arm_obj,
        outfit_map,
        OUT_LEATHER,
        "preview_leather_apron_angles.png",
        "preview_leather_apron_body_types.png",
        mode="leather",
    )
    cleanup_imported_objects(all_imp)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_LEATHER / "avental_couro_ferreiro.blend"))
    print("[done] AventalCouroFerreiro v2 complete!")


def run_cloth_apron():
    OUT_CLOTH.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, _, _ = open_clean_chartest()

    ref_meshes, all_imp = import_reference_garments([
        "CamisaLinhoClara.glb",
        "CamisaMangaCurtaBranca.glb",
        "BlusaFeminina.glb",
        "CalcaLinho.glb",
        "CalcaCurtaJoelho.glb",
        "SaiaLonga.glb",
    ])
    outfit_map = {}
    for m in ref_meshes:
        if "CamisaLinhoClara" in m.name:
            outfit_map["linen_shirt"] = m
        elif "CamisaMangaCurtaBranca" in m.name:
            outfit_map["short_shirt"] = m
        elif "BlusaFeminina" in m.name:
            outfit_map["blouse"] = m
        elif "CalcaLinho" in m.name:
            outfit_map["pants"] = m
        elif "CalcaCurtaJoelho" in m.name:
            outfit_map["knee_pants"] = m
        elif "SaiaLonga" in m.name:
            outfit_map["skirt"] = m

    under_garments = [outfit_map[k] for k in ("linen_shirt", "blouse", "pants", "skirt") if k in outfit_map]
    under_bvh, under_coords = build_composite_under_bvh(body_obj, under_garments, exclude_arms_x=0.205, exclude_head_z=1.600)

    tex_img = generate_cloth_apron_texture(TECIDO_GROSSO_PNG, OUT_CLOTH / "avental_pano_taverna_128.png")
    apron_obj, interior_idx = build_cloth_apron_mesh(under_bvh, under_coords)
    apron_obj.parent = arm_obj
    export_uv_overlay(apron_obj, OUT_CLOTH / "avental_pano_taverna_uv.png")

    autofit_garment_to_chartest(apron_obj, body_obj, arm_obj, interior_idx)
    sanitize_apron_weights(apron_obj)
    lockstep_over_clothes_morphs(apron_obj, body_obj, under_garments, interior_idx)

    verify_garment_and_export_glb(apron_obj, body_obj, arm_obj, tex_img, OUT_CLOTH / "AventalPanoTaverna.glb", interior_idx)
    audit_over_outfit_clearance(apron_obj, body_obj, arm_obj, under_garments, interior_idx, "ClothApron")

    render_apron_previews(
        apron_obj,
        body_obj,
        arm_obj,
        outfit_map,
        OUT_CLOTH,
        "preview_cloth_apron_angles.png",
        "preview_cloth_apron_body_types.png",
        mode="cloth",
    )
    cleanup_imported_objects(all_imp)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_CLOTH / "avental_pano_taverna.blend"))
    print("[done] AventalPanoTaverna v2 complete!")


if __name__ == "__main__":
    run_leather_apron()
    run_cloth_apron()
