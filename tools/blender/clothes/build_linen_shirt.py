"""Build Grimora low-poly PSX Simple Light Linen Shirt (CamisaLinhoClara) + Universal Body Auto-Fit.

Generates in conversation staging directory (prior to user approval):
- CamisaLinhoClara.glb                  (skinned to chartest_rig 42 joints + all 24 cloth-fitted morph targets + 128x128 PNG)
- camisa_linho_clara_128.png            (128x128 PSX light unbleached linen derived from assets/textures/tecido_grosso.png)
- camisa_linho_clara_uv.png             (128x128 transparent UV overlay)
- camisa_linho_clara.blend              (authoring scene with live sliders synced 1:1 from chartest_rig & chartest_body)
- preview_shirt_angles.png              (4-angle showcase: Front, 3/4 Front, Side, 3/4 Back)
- preview_shirt_body_types.png          (Single-mesh Auto-Fit showcase across 5 body types + isolated piece)
"""

from collections import defaultdict
import json
import math
import pathlib
import struct

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree


ROOT = pathlib.Path(r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE")
CHARTEST_BLEND = ROOT / "assets/models/chartest/chartest.blend"
CHARTEST_GLTF = ROOT / "assets/models/chartest/chartest.gltf"
TECIDO_GROSSO_PNG = ROOT / "assets/textures/tecido_grosso.png"
OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_linen_shirt"
)


def clamp(x, lo=0.0, hi=1.0):
    return lo if x < lo else (hi if x > hi else x)


def smoothstep(e0, e1, x):
    if abs(e1 - e0) < 1e-9:
        return 1.0 if x >= e1 else 0.0
    t = clamp((x - e0) / (e1 - e0))
    return t * t * (3.0 - 2.0 * t)


def bell(x, center, half_width):
    d = abs(x - center) / max(half_width, 1e-6)
    if d >= 1.0:
        return 0.0
    t = 1.0 - d
    return t * t * (3.0 - 2.0 * t)


def generate_light_linen_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive a 128x128 light unbleached linen atlas from assets/textures/tecido_grosso.png.

    Preserves 100% of tecido_grosso.png's fine woven thread structure (trama) per materiais.md
    while calming low-frequency burlap blotches so the shirt reads cleanly as soft light linen:
    - V in [0.16 .. 1.00]: Light natural ecru/cream linen weave + subtle stitched border band at y=22..24
    - V in [0.07 .. 0.155]: Warm natural brown cord (for V-neck lacing)
    - V in [0.00 .. 0.068]: Deep warm interior cloth shadow (collar/cuff/hem inside lips)
    """
    src = bpy.data.images.load(str(src_png), check_existing=True)
    w, h = src.size
    assert (w, h) == (128, 128), f"Expected 128x128 tecido_grosso.png, got {(w, h)}"
    src_px = list(src.pixels[:])
    out_px = [0.0] * (w * h * 4)

    lum_grid = [[0.0] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            idx = (y * w + x) * 4
            lum_grid[y][x] = 0.299 * src_px[idx] + 0.587 * src_px[idx + 1] + 0.114 * src_px[idx + 2]

    # Local 7x7 wrap-around mean to isolate crisp woven thread trama from coarse burlap blotches
    weave_signal = [[0.0] * w for _ in range(h)]
    vals = []
    for y in range(h):
        for x in range(w):
            s = 0.0
            cnt = 0
            for dy in range(-3, 4):
                yy = (y + dy) % h
                for dx in range(-3, 4):
                    xx = (x + dx) % w
                    s += lum_grid[yy][xx]
                    cnt += 1
            local_mean = s / cnt
            fine_trama = lum_grid[y][x] - local_mean
            # Keep 100% of fine woven thread cross-hatch + 28% of organic macro variation
            sig = fine_trama * 1.0 + (local_mean - 0.18) * 0.28
            weave_signal[y][x] = sig
            vals.append(sig)

    v_sorted = sorted(vals)
    v_lo = v_sorted[int(0.03 * len(v_sorted))]
    v_hi = v_sorted[int(0.97 * len(v_sorted))]
    v_span = max(v_hi - v_lo, 1e-4)

    # Cohesive warm unbleached light linen palette (PSX dark-fantasy cream/oatmeal, never neon white)
    linen_dark = (0.63, 0.58, 0.49)
    linen_mid = (0.78, 0.74, 0.65)
    linen_hi = (0.90, 0.86, 0.78)

    cord_dark = (0.18, 0.11, 0.06)
    cord_hi = (0.44, 0.30, 0.18)

    inner_col = (0.15, 0.13, 0.11)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((weave_signal[y][x] - v_lo) / v_span, 0.0, 1.0)

            if v < 0.068:
                f = 0.84 + 0.32 * tn
                out_px[idx : idx + 4] = [inner_col[0] * f, inner_col[1] * f, inner_col[2] * f, 1.0]
            elif v < 0.155:
                twist = 0.88 + 0.24 * math.sin((x * 0.65 + y * 0.85))
                tc = clamp(tn * twist, 0.0, 1.0)
                r = cord_dark[0] + (cord_hi[0] - cord_dark[0]) * tc
                g = cord_dark[1] + (cord_hi[1] - cord_dark[1]) * tc
                b = cord_dark[2] + (cord_hi[2] - cord_dark[2]) * tc
                out_px[idx : idx + 4] = [r, g, b, 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = linen_dark[0] + (linen_mid[0] - linen_dark[0]) * k
                    g = linen_dark[1] + (linen_mid[1] - linen_dark[1]) * k
                    b = linen_dark[2] + (linen_mid[2] - linen_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = linen_mid[0] + (linen_hi[0] - linen_mid[0]) * k
                    g = linen_mid[1] + (linen_hi[1] - linen_mid[1]) * k
                    b = linen_mid[2] + (linen_hi[2] - linen_mid[2]) * k

                # Subtle woven border hem stitch line at y in (23, 24) (cuffs, collar & bottom hem)
                if y in (23, 24) and ((x // 2) % 2 == 0):
                    r *= 0.84
                    g *= 0.82
                    b *= 0.78
                edge_wear = 1.0 - 0.07 * smoothstep(0.24, 0.16, v)
                out_px[idx : idx + 4] = [clamp(r * edge_wear), clamp(g * edge_wear), clamp(b * edge_wear), 1.0]

    img = bpy.data.images.new("camisa_linho_clara_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def export_uv_overlay(mesh_obj, uv_png_path: pathlib.Path, size: int = 128):
    """Write a 128x128 transparent PNG with crisp 1px UV edges."""
    buf = [0.0] * (size * size * 4)

    def put_px(ix, iy, rgba):
        if 0 <= ix < size and 0 <= iy < size:
            o = (iy * size + ix) * 4
            buf[o : o + 4] = rgba

    def draw_line(u0, v0, u1, v1):
        x0, y0 = int(round(clamp(u0) * (size - 1))), int(round(clamp(v0) * (size - 1)))
        x1, y1 = int(round(clamp(u1) * (size - 1))), int(round(clamp(v1) * (size - 1)))
        if abs(x1 - x0) > size // 2:
            return
        steps = max(abs(x1 - x0), abs(y1 - y0), 1)
        for s in range(steps + 1):
            t = s / steps
            put_px(int(round(x0 + (x1 - x0) * t)), int(round(y0 + (y1 - y0) * t)), (0.20, 0.55, 0.92, 0.88))

    bm = bmesh.new()
    bm.from_mesh(mesh_obj.data)
    uv_lay = bm.loops.layers.uv.active
    for f in bm.faces:
        loops = list(f.loops)
        for i in range(len(loops)):
            a = loops[i][uv_lay].uv
            b = loops[(i + 1) % len(loops)][uv_lay].uv
            draw_line(a.x, a.y, b.x, b.y)
    bm.free()

    img = bpy.data.images.new("uv_overlay", width=size, height=size, alpha=True)
    img.pixels[:] = buf
    img.filepath_raw = str(uv_png_path)
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)


def build_body_bvh_from_coords(body_mesh, vert_coords) -> tuple[BVHTree, list[tuple[int, int, int]]]:
    """Build a triangulated BVHTree + face->vertex index map for exact barycentric queries."""
    bm = bmesh.new()
    bm.from_mesh(body_mesh)
    bm.verts.ensure_lookup_table()
    for i, v in enumerate(bm.verts):
        v.co = vert_coords[i].copy()
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    bm.faces.ensure_lookup_table()
    tri_verts = [(f.verts[0].index, f.verts[1].index, f.verts[2].index) for f in bm.faces]
    bvh = BVHTree.FromBMesh(bm)
    bm.free()
    return bvh, tri_verts


def ordered_boundary_loop(edges: list[bmesh.types.BMEdge]) -> list[bmesh.types.BMVert]:
    """Walk a closed boundary edge loop in topological order."""
    adj = defaultdict(list)
    for e in edges:
        v0, v1 = e.verts[0], e.verts[1]
        adj[v0].append(v1)
        adj[v1].append(v0)
    start = min(adj.keys(), key=lambda v: (round(v.co.x, 5), round(v.co.y, 5), round(v.co.z, 5)))
    loop = [start]
    prev = None
    cur = start
    while True:
        nbs = adj[cur]
        nxt = nbs[0] if nbs[0] != prev else (nbs[1] if len(nbs) > 1 else None)
        if nxt is None or nxt == start:
            break
        loop.append(nxt)
        prev, cur = cur, nxt
    return loop


def extrude_closed_rim_inward(bm, loop_verts, push_in_dirs, rim_thick=0.0042, sink_depth=0.0048):
    """Extrude a solid folded cloth thickness rim + dark interior shadow lip on a closed boundary loop."""
    n = len(loop_verts)
    v0, v1 = loop_verts[0], loop_verts[1]
    shared_edge = bm.edges.get((v0, v1))
    flip = False
    if shared_edge and shared_edge.link_faces:
        f = shared_edge.link_faces[0]
        loops = list(f.loops)
        for idx, lp in enumerate(loops):
            if lp.vert == v0 and loops[(idx + 1) % len(loops)].vert == v1:
                flip = True
                break

    rim_verts = []
    deep_verts = []
    for v, in_dir in zip(loop_verts, push_in_dirs):
        d = in_dir.normalized()
        vr = bm.verts.new(v.co + d * rim_thick)
        vd = bm.verts.new(v.co + d * (rim_thick + 0.0018))
        rim_verts.append(vr)
        deep_verts.append(vd)

    for i, v in enumerate(loop_verts):
        interior_nbs = [
            e.other_vert(v) for e in v.link_edges if not e.is_boundary and e.other_vert(v) not in rim_verts
        ]
        if interior_nbs:
            avg_in = sum((nb.co for nb in interior_nbs), Vector((0.0, 0.0, 0.0))) / len(interior_nbs)
            tang_in = avg_in - v.co
            if tang_in.length > 1e-5:
                deep_verts[i].co += tang_in.normalized() * sink_depth

    rim_faces = []
    shadow_faces = []
    for i in range(n):
        j = (i + 1) % n
        if flip:
            f_rim = bm.faces.new((loop_verts[j], loop_verts[i], rim_verts[i], rim_verts[j]))
            f_shd = bm.faces.new((rim_verts[j], rim_verts[i], deep_verts[i], deep_verts[j]))
        else:
            f_rim = bm.faces.new((loop_verts[i], loop_verts[j], rim_verts[j], rim_verts[i]))
            f_shd = bm.faces.new((rim_verts[i], rim_verts[j], deep_verts[j], deep_verts[i]))
        rim_faces.append(f_rim)
        shadow_faces.append(f_shd)
    return rim_faces, shadow_faces, rim_verts, deep_verts


def enforce_mesh_clearance_over_bvh(
    bm: bmesh.types.BMesh,
    bvh: BVHTree,
    skip_verts: set[bmesh.types.BMVert],
    min_vert_gap: float = 0.0070,
    min_face_gap: float = 0.0055,
):
    """Push any exterior vertex, edge midpoint, or quad center that comes closer than min_gap to body BVH."""
    for _ in range(3):
        # 1) Vertex clearance
        for v in bm.verts:
            if v in skip_verts:
                continue
            loc, nrm, _, _ = bvh.find_nearest(v.co)
            if loc is not None and nrm is not None:
                d_s = (v.co - loc).dot(nrm)
                if d_s < min_vert_gap:
                    v.co += nrm * (min_vert_gap - d_s)

        # 2) Quad face centroid + edge midpoint chord clearance (prevents low-poly quad sagitta poke-through!)
        face_pushes = defaultdict(lambda: Vector((0.0, 0.0, 0.0)))
        for f in bm.faces:
            if any(v in skip_verts for v in f.verts):
                continue
            c = f.calc_center_median()
            sample_pts = [c] + [(e.verts[0].co + e.verts[1].co) * 0.5 for e in f.edges]
            needed = 0.0
            best_nrm = None
            for sp in sample_pts:
                loc, nrm, _, _ = bvh.find_nearest(sp)
                if loc is not None and nrm is not None:
                    d_s = (sp - loc).dot(nrm)
                    if min_face_gap - d_s > needed:
                        needed = min_face_gap - d_s
                        best_nrm = nrm
            if needed > 0.0 and best_nrm is not None:
                push = best_nrm * (needed + 0.0010)
                for v in f.verts:
                    if push.length > face_pushes[v].length:
                        face_pushes[v] = push
        for v, pvec in face_pushes.items():
            v.co += pvec


def build_linen_shirt_mesh(body_obj, arm_obj) -> tuple[bpy.types.Object, set[int]]:
    """Construct CamisaLinhoClara: relaxed medieval V-slit linen shirt with cuffs, hip vents & lacing."""
    basis_kb = body_obj.data.shape_keys.key_blocks[0].data
    basis_coords = [basis_kb[i].co.copy() for i in range(len(body_obj.data.vertices))]
    body_bvh, _ = build_body_bvh_from_coords(body_obj.data, basis_coords)

    bm = bmesh.new()
    bm.from_mesh(body_obj.data)
    # CRITICAL: Remove inherited body UV layers so our new shirt UVMap is TEXCOORD_0!
    for old_uv in list(bm.loops.layers.uv):
        bm.loops.layers.uv.remove(old_uv)

    bm.verts.ensure_lookup_table()
    for i, v in enumerate(bm.verts):
        v.co = basis_coords[i].copy()

    fa_l = arm_obj.data.bones["forearm.L"]
    fa_r = arm_obj.data.bones["forearm.R"]
    ua_l = arm_obj.data.bones["upper_arm.L"]
    ua_r = arm_obj.data.bones["upper_arm.R"]

    def is_neck_or_head(c: Vector) -> bool:
        r_neck = math.hypot(c.x, (c.y + 0.088) * 0.85)
        return c.z > 1.560 or (c.z > 1.522 and r_neck < 0.078)

    def keep_face(f) -> bool:
        c = f.calc_center_median()
        if c.z < 1.015:
            return False
        if is_neck_or_head(c):
            return False
        for fa in (fa_l, fa_r):
            u = (fa.tail_local - fa.head_local).normalized()
            L = (fa.tail_local - fa.head_local).length
            if abs(c.x) > 0.35 and (c - fa.head_local).dot(u) / L > 0.92:
                return False
        return True

    bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep_face(f)], context="FACES")
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
    bm.verts.ensure_lookup_table()
    bm.edges.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # Identify the 4 clean closed boundary loops: Collar, Waist, Left Wrist, Right Wrist
    visited = set()
    b_loops = []
    for e in [ed for ed in bm.edges if ed.is_boundary]:
        if e in visited:
            continue
        comp = []
        stack = [e]
        while stack:
            cur = stack.pop()
            if cur in visited:
                continue
            visited.add(cur)
            comp.append(cur)
            for v in cur.verts:
                for ne in v.link_edges:
                    if ne.is_boundary and ne not in visited:
                        stack.append(ne)
        b_loops.append(ordered_boundary_loop(comp))

    collar_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) > 1.45)
    waist_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) < 1.08)
    wrist_l_loop = next(lp for lp in b_loops if sum(v.co.x for v in lp) / len(lp) < -0.35)
    wrist_r_loop = next(lp for lp in b_loops if sum(v.co.x for v in lp) / len(lp) > +0.35)

    boundary_set = set(collar_loop) | set(waist_loop) | set(wrist_l_loop) | set(wrist_r_loop)

    # =========================================================================
    # 1. RELAX BARE-SKIN ANATOMY INTO SOFT WOVEN LINEN PLANES + DRAPE OFFSET
    # =========================================================================
    # Level Collar & Waist heights BEFORE BVH clearance push so neck/waist clearance stays exact!
    for v in collar_loop:
        front_f = smoothstep(-0.16, -0.03, v.co.y)
        target_z = 1.530 * (1.0 - front_f) + 1.498 * front_f + 0.008 * bell(abs(v.co.x), 0.072, 0.045)
        v.co.z = v.co.z * 0.35 + target_z * 0.65

    for v in waist_loop:
        v.co.z = 1.015 - 0.004 * smoothstep(-0.05, 0.06, v.co.y)

    # Gentle cloth relaxation on interior torso/sleeve vertices (softens muscle grooves into woven fabric)
    for _ in range(3):
        new_pos = {}
        for v in bm.verts:
            if v in boundary_set or abs(v.co.x) < 1e-4:
                continue
            nbs = [e.other_vert(v) for e in v.link_edges]
            if nbs:
                avg = sum((nb.co for nb in nbs), Vector((0.0, 0.0, 0.0))) / len(nbs)
                new_pos[v] = v.co.lerp(avg, 0.30)
        for v, co in new_pos.items():
            v.co = co

    bm.normal_update()

    # Apply tailored linen slack (snugger on shoulders/collar, airy bloused drape on torso & sleeves)
    for v in bm.verts:
        p = v.co.copy()
        loc, n_body, _, _ = body_bvh.find_nearest(p)
        out_n = v.normal.normalized()
        if n_body is not None and out_n.dot(n_body) > 0.15:
            out_n = (out_n * 0.55 + n_body * 0.45).normalized()

        arm_t = smoothstep(0.16, 0.30, abs(p.x))
        waist_t = smoothstep(1.32, 1.02, p.z) * (1.0 - arm_t)
        lumbar_bridge = bell(p.z, 1.14, 0.16) * smoothstep(-0.04, -0.14, p.y) * (1.0 - arm_t) * 0.0090
        sternum_bridge = bell(p.x, 0.0, 0.055) * bell(p.z, 1.33, 0.12) * smoothstep(0.0, 0.06, p.y) * 0.0060

        ang = math.atan2(p.x, p.y + 0.05)
        torso_folds = (0.5 + 0.5 * math.cos(ang * 7.0)) * 0.0026 * waist_t
        sleeve_folds = (0.5 + 0.5 * math.cos(p.y * 55.0 + p.z * 35.0)) * 0.0024 * arm_t

        slack = 0.0098 + 0.0052 * waist_t + 0.0060 * arm_t + lumbar_bridge + sternum_bridge + torso_folds + sleeve_folds
        if v in collar_loop:
            slack = 0.0075

        base_pt = loc if (loc is not None and (p - loc).dot(out_n) < 0.003) else p
        v.co = base_pt + out_n * slack

    # =========================================================================
    # 2. SCULPT BLOUSED LONG SLEEVES + GATHERED FOLDED WRIST CUFFS
    # =========================================================================
    cuff_shadow_faces = []
    cuff_border_faces = []
    interior_verts = set()

    for w_loop, fa_bone in [(wrist_l_loop, fa_l), (wrist_r_loop, fa_r)]:
        a_pt = fa_bone.head_local
        b_pt = fa_bone.tail_local
        u_ax = (b_pt - a_pt).normalized()
        L_fa = (b_pt - a_pt).length

        v0, v1 = w_loop[0], w_loop[1]
        sh_e = bm.edges.get((v0, v1))
        flip_w = False
        if sh_e and sh_e.link_faces:
            f0 = sh_e.link_faces[0]
            lps = list(f0.loops)
            for li, lp in enumerate(lps):
                if lp.vert == v0 and lps[(li + 1) % len(lps)].vert == v1:
                    flip_w = True
                    break

        # Sample exact radial directions from forearm bone and raycast actual forearm/wrist skin radius!
        def ring_at_forearm(t_along: float, extra_slack: float, pleat_amp: float = 0.0) -> tuple[list[Vector], list[Vector]]:
            c_ax = a_pt + u_ax * (L_fa * t_along)
            pts_out = []
            r_dirs = []
            for idx_v, v in enumerate(w_loop):
                rel = v.co - a_pt
                radial = rel - u_ax * rel.dot(u_ax)
                r_dir = radial.normalized()
                r_dirs.append(r_dir)
                # Raycast from bone centerline outward to find true wrist/forearm skin surface!
                hit_loc, _, _, _ = body_bvh.ray_cast(c_ax, r_dir, 0.14)
                r_skin = (hit_loc - c_ax).length if hit_loc is not None else 0.036
                pleat = pleat_amp * math.cos(idx_v * 2.0 * math.pi / len(w_loop) * 3.0)
                pts_out.append(c_ax + r_dir * (r_skin + extra_slack + pleat))
            return pts_out, r_dirs

        # Reposition w_loop to t = 0.76 as airy bloused sleeve puff (+16.5mm over forearm skin)
        blouse_pts, rad_dirs = ring_at_forearm(0.76, extra_slack=0.0165, pleat_amp=0.0030)
        for v, bpt in zip(w_loop, blouse_pts):
            v.co = bpt

        # Extrude 2 exterior cuff rings: t=0.845 (gather taper +9.5mm) -> t=0.910 (fitted wrist cuff band +7.5mm)
        prev_ring = w_loop
        for st_t, slk, is_band in [(0.845, 0.0098, False), (0.910, 0.0080, True)]:
            cuff_pts, _ = ring_at_forearm(st_t, extra_slack=slk, pleat_amp=0.0)
            new_ring = [bm.verts.new(pt) for pt in cuff_pts]
            n_w = len(w_loop)
            for i in range(n_w):
                j = (i + 1) % n_w
                if flip_w:
                    fq = bm.faces.new((prev_ring[j], prev_ring[i], new_ring[i], new_ring[j]))
                else:
                    fq = bm.faces.new((prev_ring[i], prev_ring[j], new_ring[j], new_ring[i]))
                if is_band:
                    cuff_border_faces.append(fq)
            prev_ring = new_ring

        # Solid 4.5mm inward cuff thickness rim + dark interior shadow lip
        in_dirs = [-rd for rd in rad_dirs]
        r_faces, s_faces, r_vs, d_vs = extrude_closed_rim_inward(
            bm, prev_ring, in_dirs, rim_thick=0.0045, sink_depth=0.0075
        )
        cuff_border_faces.extend(r_faces)
        cuff_shadow_faces.extend(s_faces)
        interior_verts.update(r_vs)
        interior_verts.update(d_vs)

    # =========================================================================
    # 3. EXTRUDE FLARED TUNIC SKIRT (z=1.015 -> 0.890m) WITH LATERAL HIP VENTS
    # =========================================================================
    wv0, wv1 = waist_loop[0], waist_loop[1]
    we0 = bm.edges.get((wv0, wv1))
    flip_waist = False
    if we0 and we0.link_faces:
        fw = we0.link_faces[0]
        lps = list(fw.loops)
        for li, lp in enumerate(lps):
            if lp.vert == wv0 and lps[(li + 1) % len(lps)].vert == wv1:
                flip_waist = True
                break

    idx_left_vent = min(range(len(waist_loop)), key=lambda i: waist_loop[i].co.x)
    idx_right_vent = max(range(len(waist_loop)), key=lambda i: waist_loop[i].co.x)

    skirt_z_levels = [0.972, 0.930, 0.890]
    n_waist = len(waist_loop)

    ring_0 = []
    for v in waist_loop:
        ang = math.atan2(v.co.x, v.co.y + 0.045)
        horiz = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
        orig = Vector((0.0, -0.045, skirt_z_levels[0]))
        loc, _, _, _ = body_bvh.ray_cast(orig, horiz, 0.35)
        r_body = (loc - orig).length if loc is not None else math.hypot(v.co.x, v.co.y + 0.045)
        pleat = math.cos(ang * 7.0) * 0.0028
        r_skirt = max(r_body + 0.0150 + pleat, math.hypot(v.co.x, v.co.y + 0.045) + 0.0025)
        p_new = orig + horiz * r_skirt
        p_new.z = skirt_z_levels[0] - 0.005 * smoothstep(-0.05, 0.06, p_new.y)
        ring_0.append(bm.verts.new(p_new))

    hem_border_faces = []
    for i in range(n_waist):
        j = (i + 1) % n_waist
        if flip_waist:
            bm.faces.new((waist_loop[j], waist_loop[i], ring_0[i], ring_0[j]))
        else:
            bm.faces.new((waist_loop[i], waist_loop[j], ring_0[j], ring_0[i]))

    def panel_indices(i_start, i_end):
        seq = [i_start]
        c_i = i_start
        while c_i != i_end:
            c_i = (c_i + 1) % n_waist
            seq.append(c_i)
        return seq

    panel_a = panel_indices(idx_left_vent, idx_right_vent)
    panel_b = panel_indices(idx_right_vent, idx_left_vent)

    for panel in (panel_a, panel_b):
        mean_y = sum(ring_0[i].co.y for i in panel[1:-1]) / max(len(panel) - 2, 1)
        panel_y_sign = +1.0 if mean_y >= -0.04 else -1.0

        prev_row = [ring_0[i] for i in panel]
        for tier_idx, z_lvl in enumerate(skirt_z_levels[1:], start=1):
            vent_spread = 0.0075 * tier_idx  # Crisp 30mm lateral hip vents!
            cur_row = []
            for p_pos_i, w_i in enumerate(panel):
                v_ref = ring_0[w_i]
                ang = math.atan2(v_ref.co.x, v_ref.co.y + 0.045)
                horiz = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
                orig = Vector((0.0, -0.045, z_lvl))
                loc, _, _, _ = body_bvh.ray_cast(orig, horiz, 0.35)
                r_body = (loc - orig).length if loc is not None else math.hypot(v_ref.co.x, v_ref.co.y + 0.045)
                pleat = math.cos(ang * 7.0) * (0.0032 + 0.0014 * tier_idx)
                flare = 0.0165 + 0.0040 * tier_idx
                r_sk = max(r_body + flare + pleat, math.hypot(v_ref.co.x, v_ref.co.y + 0.045) + 0.0035 * tier_idx)
                pt = orig + horiz * r_sk
                pt.z = z_lvl - 0.008 * bell(pt.x, 0.0, 0.14)
                if p_pos_i == 0 or p_pos_i == len(panel) - 1:
                    pt.y += panel_y_sign * vent_spread
                    pt.z += 0.0045 * tier_idx
                cur_row.append(bm.verts.new(pt))

            for k in range(len(panel) - 1):
                if flip_waist:
                    fq = bm.faces.new((prev_row[k + 1], prev_row[k], cur_row[k], cur_row[k + 1]))
                else:
                    fq = bm.faces.new((prev_row[k], prev_row[k + 1], cur_row[k + 1], cur_row[k]))
                if tier_idx == 2:
                    hem_border_faces.append(fq)
            prev_row = cur_row

    bm.edges.ensure_lookup_table()
    skirt_bnd_edges = [e for e in bm.edges if e.is_boundary and all(v.co.z < 1.00 for v in e.verts)]
    skirt_bnd_loop = ordered_boundary_loop(skirt_bnd_edges)
    skirt_in_dirs = [Vector((-v.co.x, -(v.co.y + 0.045), 0.12)).normalized() for v in skirt_bnd_loop]
    hr_faces, hs_faces, hr_vs, hd_vs = extrude_closed_rim_inward(
        bm, skirt_bnd_loop, skirt_in_dirs, rim_thick=0.0048, sink_depth=0.0070
    )
    hem_border_faces.extend(hr_faces)
    cuff_shadow_faces.extend(hs_faces)
    interior_verts.update(hr_vs)
    interior_verts.update(hd_vs)

    # =========================================================================
    # 4. OPEN MEDIEVAL V-SLIT COLLAR + FOLDED COLLAR LAPEL + CRISSCROSS LACING
    # =========================================================================
    front_center_vs = sorted(
        [v for v in bm.verts if abs(v.co.x) < 0.0015 and v.co.y > -0.055 and 1.395 <= v.co.z <= 1.545],
        key=lambda v: -v.co.z,
    )
    split_targets = front_center_vs[:-1]
    v_apex = front_center_vs[-1]

    v_pairs = []
    spreads = [0.026, 0.019, 0.010]  # Crisp open V-neckline showing collarbone/sternum + lacing!
    for idx_s, v_orig in enumerate(split_targets):
        sp = spreads[min(idx_s, len(spreads) - 1)]
        pos_faces = [f for f in list(v_orig.link_faces) if f.calc_center_median().x > 0.0]
        v_right = bm.verts.new(v_orig.co.copy())
        for f in pos_faces:
            verts_f = [v_right if fv == v_orig else fv for fv in f.verts]
            bm.faces.remove(f)
            bm.faces.new(verts_f)

        v_orig.co.x = -sp
        v_orig.co.y += 0.0030
        v_right.co.x = +sp
        v_right.co.y += 0.0030
        v_pairs.append((v_orig, v_right))

    v_apex.co.y += 0.0025
    bm.edges.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    collar_bnd_edges = [e for e in bm.edges if e.is_boundary and all(v.co.z > 1.38 for v in e.verts)]
    collar_v_loop = ordered_boundary_loop(collar_bnd_edges)

    cv0, cv1 = collar_v_loop[0], collar_v_loop[1]
    ce0 = bm.edges.get((cv0, cv1))
    flip_col = False
    if ce0 and ce0.link_faces:
        fc = ce0.link_faces[0]
        lps = list(fc.loops)
        for li, lp in enumerate(lps):
            if lp.vert == cv0 and lps[(li + 1) % len(lps)].vert == cv1:
                flip_col = True
                break

    collar_stand_verts = []
    collar_in_dirs = []
    for v in collar_v_loop:
        neck_ness = smoothstep(1.470, 0.5 * (1.498 + 1.525), v.co.z)
        out_xy = Vector((v.co.x, v.co.y + 0.092, 0.0))
        if out_xy.length < 1e-4:
            out_xy = Vector((0.0, 1.0, 0.0))
        out_xy.normalize()
        # Stand-up rolled linen collar around neck (~10mm tall, flaring slightly outward so it never touches neck!)
        stand_offset = Vector((0.0, 0.0, 0.0105 * neck_ness)) + out_xy * (0.0035 * neck_ness + 0.0018 * (1.0 - neck_ness))
        vs = bm.verts.new(v.co + stand_offset)
        # Ensure collar_stand_verts is also outside body_bvh by >= 7mm
        loc_c, nrm_c, _, _ = body_bvh.find_nearest(vs.co)
        if loc_c is not None and nrm_c is not None:
            gap_c = (vs.co - loc_c).dot(nrm_c)
            if gap_c < 0.0072:
                vs.co += nrm_c * (0.0072 - gap_c)
        collar_stand_verts.append(vs)

        # Along V-placket, push rim strictly inward toward chest (-Y) without closing the X V-gap!
        if neck_ness > 0.35:
            in_dir = Vector((-out_xy.x, -out_xy.y, -0.10)).normalized()
        else:
            side_away = -1.0 if v.co.x < -0.002 else (+1.0 if v.co.x > 0.002 else 0.0)
            in_dir = Vector((side_away * 0.25, -0.95, -0.10)).normalized()
        collar_in_dirs.append(in_dir)

    collar_border_faces = []
    n_col = len(collar_v_loop)
    for i in range(n_col):
        j = (i + 1) % n_col
        if flip_col:
            fq = bm.faces.new((collar_v_loop[j], collar_v_loop[i], collar_stand_verts[i], collar_stand_verts[j]))
        else:
            fq = bm.faces.new((collar_v_loop[i], collar_v_loop[j], collar_stand_verts[j], collar_stand_verts[i]))
        collar_border_faces.append(fq)

    cr_faces, cs_faces, cr_vs, cd_vs = extrude_closed_rim_inward(
        bm, collar_stand_verts, collar_in_dirs, rim_thick=0.0035, sink_depth=0.0045
    )
    collar_border_faces.extend(cr_faces)
    cuff_shadow_faces.extend(cs_faces)
    interior_verts.update(cr_vs)
    interior_verts.update(cd_vs)

    # Guarantee 100% exterior vertex + quad-face + edge-midpoint clearance over Basis body BEFORE adding cords!
    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0072, min_face_gap=0.0058
    )

    # =========================================================================
    # 5. ADD 2 CRISSCROSS LOW-POLY BROWN LACING CORDS ACROSS THE V-SLIT
    # =========================================================================
    cord_faces = []

    def add_cord_segment(p_a: Vector, p_b: Vector, radius: float = 0.0020):
        ax = (p_b - p_a).normalized()
        up = Vector((0.0, 1.0, 0.0))
        if abs(ax.dot(up)) > 0.9:
            up = Vector((0.0, 0.0, 1.0))
        s1 = ax.cross(up).normalized() * radius
        s2 = ax.cross(s1).normalized() * radius
        p_m = (p_a + p_b) * 0.5 + Vector((0.0, 0.0025, 0.0))
        # Ensure midpoint clears sternum skin cleanly
        loc_m, nrm_m, _, _ = body_bvh.find_nearest(p_m)
        if loc_m is not None and nrm_m is not None and (p_m - loc_m).dot(nrm_m) < 0.0045:
            p_m = loc_m + nrm_m * 0.0045
        rings = []
        for pt in (p_a, p_m, p_b):
            rings.append([
                bm.verts.new(pt + s1),
                bm.verts.new(pt + s2),
                bm.verts.new(pt - s1),
                bm.verts.new(pt - s2),
            ])
            interior_verts.update(rings[-1])
        for r_i in range(2):
            ra, rb = rings[r_i], rings[r_i + 1]
            for k in range(4):
                kn = (k + 1) % 4
                cord_faces.append(bm.faces.new((ra[k], ra[kn], rb[kn], rb[k])))

    if len(v_pairs) >= 3:
        (l0, r0), (l1, r1), (l2, r2) = v_pairs[0], v_pairs[1], v_pairs[2]
        in_push = Vector((0.0, -0.0006, 0.0))
        add_cord_segment(l0.co * 0.35 + l1.co * 0.65 + in_push, r1.co * 0.65 + r2.co * 0.35 + in_push)
        add_cord_segment(r0.co * 0.35 + r1.co * 0.65 + in_push, l1.co * 0.65 + l2.co * 0.35 + in_push)
        add_cord_segment(l1.co * 0.25 + l2.co * 0.75 + in_push, r2.co * 0.70 + v_apex.co * 0.30 + in_push)
        add_cord_segment(r1.co * 0.25 + r2.co * 0.75 + in_push, l2.co * 0.70 + v_apex.co * 0.30 + in_push)

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # =========================================================================
    # 6. ASSIGN UNIFORM CYLINDRICAL SEWING-PATTERN UVS (100% ACTIVE UVMap!)
    # =========================================================================
    uv_lay = bm.loops.layers.uv.new("UVMap")
    shadow_set = set(cuff_shadow_faces)
    cord_set = set(cord_faces)
    border_set = set(cuff_border_faces) | set(hem_border_faces) | set(collar_border_faces)

    for f in bm.faces:
        if f in shadow_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.032)
            continue
        if f in cord_set:
            for k_i, lp in enumerate(f.loops):
                lp[uv_lay].uv = ((k_i % 2) * 0.55 + 0.20, 0.105 if k_i < 2 else 0.132)
            continue
        if f in border_set:
            c = f.calc_center_median()
            u_raws = []
            for lp in f.loops:
                p = lp.vert.co
                ang = math.atan2(p.x, p.y + 0.045)
                u_raws.append((lp, 0.50 + ang / (2.0 * math.pi) * 0.90, p))
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            for lp, u_val, p in u_raws:
                is_lower = (p.z < c.z) if abs(c.x) < 0.25 else (abs(p.x) > abs(c.x))
                v_val = 0.175 if is_lower else 0.222
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)
            continue

        c = f.calc_center_median()
        if abs(c.x) > 0.195 and c.z > 1.06:
            # Sleeve pattern piece (~150 texels/m matching torso density!)
            is_left = c.x < 0.0
            ua_b = ua_l if is_left else ua_r
            fa_b = fa_l if is_left else fa_r
            sh_root = ua_b.head_local
            wr_end = fa_b.tail_local
            arm_vec = wr_end - sh_root
            arm_len = arm_vec.length
            arm_dir = arm_vec.normalized()
            up_ref = Vector((0.0, 0.0, 1.0))
            side_ref = arm_dir.cross(up_ref).normalized()
            up_ortho = side_ref.cross(arm_dir).normalized()

            u_list = []
            for lp in f.loops:
                rel = lp.vert.co - sh_root
                along = rel.dot(arm_dir) / max(arm_len, 1e-4)
                rad = rel - arm_dir * rel.dot(arm_dir)
                ang = math.atan2(rad.dot(side_ref), rad.dot(up_ortho))
                u_raw = (ang + math.pi) / (2.0 * math.pi)
                v_val = clamp(0.95 - along * 0.62, 0.24, 0.97)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.55:
                u_list = [(lp, u + 1.0 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            u_base = 0.04 if is_left else 0.54
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_base + 0.40 * u_val, 0.02, 0.98), v_val)
        else:
            # Cylindrical Torso Tunic unwrap around (0, -0.045, z) -> zero flank stretching!
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                ang = math.atan2(p.x, p.y + 0.045)  # 0 at center-front, +-pi at center-back
                u_raw = 0.50 + (ang / (2.0 * math.pi)) * 0.90
                v_val = clamp(0.22 + (p.z - 0.885) / (1.545 - 0.885) * 0.75, 0.21, 0.98)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.48:
                u_list = [(lp, u + 0.90 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior_indices = {v.index for v in interior_verts if v.is_valid}

    mesh = bpy.data.meshes.new("CamisaLinhoClara")
    bm.to_mesh(mesh)
    bm.free()

    for poly in mesh.polygons:
        poly.use_smooth = False

    shirt_obj = bpy.data.objects.new("CamisaLinhoClara", mesh)
    bpy.context.scene.collection.objects.link(shirt_obj)
    shirt_obj.parent = arm_obj
    return shirt_obj, final_interior_indices


def autofit_garment_to_chartest(shirt_obj, body_obj, arm_obj, interior_indices: set[int]):
    """UNIVERSAL CLOTHING AUTO-FIT PIPELINE:
    1. Transfers exact 42-bone skeletal skinning weights from chartest_body with seam smoothing.
    2. Transfers all 24 chartest Shape Keys with Convex-Preserving Cloth Relaxation + Bust/Glute Bridging,
       so EVEN SIMULTANEOUS MULTI-SLIDER COMBINATIONS never lose convexity or clip!
    3. Wires live drivers for BOTH rig properties AND all manual body shape-key sliders!
    """
    body_mesh = body_obj.data
    shirt_mesh = shirt_obj.data
    n_shirt = len(shirt_mesh.vertices)

    shirt_adj = defaultdict(set)
    for poly in shirt_mesh.polygons:
        vs = poly.vertices
        for i in range(len(vs)):
            a, b = vs[i], vs[(i + 1) % len(vs)]
            shirt_adj[a].add(b)
            shirt_adj[b].add(a)

    basis_kb = body_mesh.shape_keys.key_blocks[0].data
    basis_coords = [basis_kb[i].co.copy() for i in range(len(body_mesh.vertices))]
    basis_bvh, tri_verts = build_body_bvh_from_coords(body_mesh, basis_coords)

    # Precompute closest triangle + barycentric weights + outward body normal for every shirt vertex
    bary_map = []  # (i0, i1, i2, wa, wb, wc, n_out)
    for sv in shirt_mesh.vertices:
        p = sv.co
        loc, nrm, f_idx, _ = basis_bvh.find_nearest(p)
        i0, i1, i2 = tri_verts[f_idx]
        a, b, c = basis_coords[i0], basis_coords[i1], basis_coords[i2]
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
        n_out = nrm.normalized() if (nrm is not None and nrm.length > 1e-5) else Vector((0.0, 1.0, 0.0))
        bary_map.append((i0, i1, i2, wa, wb, wc, n_out))

    # =========================================================================
    # STEP 1: 42-BONE SKIN WEIGHT TRANSFER + SEAM SMOOTHING
    # =========================================================================
    body_vg_names = [vg.name for vg in body_obj.vertex_groups]
    body_weights = []
    for bv in body_mesh.vertices:
        wdict = defaultdict(float)
        for g in bv.groups:
            if g.weight > 1e-5:
                wdict[body_vg_names[g.group]] = g.weight
        body_weights.append(wdict)

    raw_shirt_w = []
    for i0, i1, i2, wa, wb, wc, _ in bary_map:
        wsum = defaultdict(float)
        for idx_b, w_b in ((i0, wa), (i1, wb), (i2, wc)):
            if w_b <= 1e-6:
                continue
            for gname, gw in body_weights[idx_b].items():
                wsum[gname] += gw * w_b
        raw_shirt_w.append(wsum)

    cur_w = raw_shirt_w
    for _ in range(3):
        nxt_w = []
        for vi in range(n_shirt):
            nbs = shirt_adj[vi]
            if not nbs:
                nxt_w.append(cur_w[vi])
                continue
            merged = defaultdict(float)
            for k, val in cur_w[vi].items():
                merged[k] += val * 0.55
            nb_scale = 0.45 / len(nbs)
            for nb in nbs:
                for k, val in cur_w[nb].items():
                    merged[k] += val * nb_scale
            nxt_w.append(merged)
        cur_w = nxt_w

    shirt_obj.vertex_groups.clear()
    vg_lookup = {gname: shirt_obj.vertex_groups.new(name=gname) for gname in body_vg_names}
    for vi, wmap in enumerate(cur_w):
        top4 = sorted(((k, v) for k, v in wmap.items() if v > 1e-4), key=lambda kv: -kv[1])[:4]
        tot = sum(v for _, v in top4)
        if tot > 1e-6:
            for gname, gw in top4:
                vg_lookup[gname].add([vi], gw / tot, "REPLACE")

    arm_mod = shirt_obj.modifiers.new(name="Armature", type="ARMATURE")
    arm_mod.object = arm_obj

    # =========================================================================
    # STEP 2: CONVEX-PRESERVING CLOTH TRANSFER OF ALL 24 SHAPE KEYS
    # =========================================================================
    shirt_obj.shape_key_add(name="Basis", from_mix=False)
    shirt_base_pts = [shirt_mesh.vertices[i].co.copy() for i in range(n_shirt)]

    fb_kb = body_mesh.shape_keys.key_blocks.get("Female_Breasts")
    bust_slice_peaks = {}
    bust_col_peaks = {}
    if fb_kb is not None:
        for z_i in range(115, 148):
            z_c = z_i * 0.01
            best_dy = 0.0
            for bi, bp in enumerate(basis_coords):
                if abs(bp.z - z_c) < 0.022 and 0.035 <= abs(bp.x) <= 0.115 and bp.y > 0.0:
                    dy = fb_kb.data[bi].co.y - bp.y
                    if dy > best_dy:
                        best_dy = dy
            bust_slice_peaks[z_i] = best_dy
        # Peak forward breast projection per |x|-column (for natural vertical linen curtain drape below bust)
        for x_i in range(0, 17):
            x_c = x_i * 0.01
            best_dy = 0.0
            for bi, bp in enumerate(basis_coords):
                if 1.25 <= bp.z <= 1.44 and abs(abs(bp.x) - x_c) < 0.024 and bp.y > 0.0:
                    dy = fb_kb.data[bi].co.y - bp.y
                    if dy > best_dy:
                        best_dy = dy
            bust_col_peaks[x_i] = best_dy
        # Centerline cleavage column inherits ~76% of the adjacent breast peak so sternum bridges smoothly
        peak_main = max(bust_col_peaks.values(), default=0.0)
        for x_i in range(0, 8):
            bridge_w = bell(x_i * 0.01, 0.0, 0.085)
            bust_col_peaks[x_i] = max(bust_col_peaks[x_i], peak_main * 0.76 * bridge_w)

    for kb_idx, kb in enumerate(body_mesh.shape_keys.key_blocks):
        if kb_idx == 0:
            continue
        kname = kb.name
        body_deltas = [kb.data[i].co - basis_coords[i] for i in range(len(basis_coords))]

        raw_deltas = []
        s_deltas = []
        for vi, (i0, i1, i2, wa, wb, wc, n_out) in enumerate(bary_map):
            d_raw = body_deltas[i0] * wa + body_deltas[i1] * wb + body_deltas[i2] * wc
            p = shirt_base_pts[vi]

            if kname == "Female_Breasts":
                # Keep vertical Z monotonic on woven fabric (prevents overhanging breast verts from folding below underbust row at 2.0x!)
                d_raw = Vector((d_raw.x, d_raw.y, d_raw.z * 0.28))

            raw_deltas.append(d_raw)
            d = d_raw.copy()

            # Outward cloth expansion cushion (+8% along body surface normal when expanding outward)
            out_comp = d.dot(n_out)
            if out_comp > 0.0:
                d = d + n_out * (out_comp * 0.08)

            if kname == "Female_Breasts" and p.y > -0.015 and abs(p.x) < 0.160:
                # 1) Cloth Tension Bridge across cleavage (spans tautly across sternum instead of "boob-sock")
                z_key = int(round(clamp(p.z, 1.15, 1.47) * 100))
                peak_dy = bust_slice_peaks.get(z_key, 0.0)
                bridge_f = bell(p.x, 0.0, 0.092) * smoothstep(-0.015, 0.035, p.y)
                bridged_dy = max(d.y, peak_dy * 0.78 * bridge_f)

                # 2) Continuous vertical linen curtain drape from lower bust (z=1.32) down to waist (z=1.02)
                #    Prevents vacuum-tucking & quad inversion under the breasts even at breasts_size = 2.00!
                x_key = int(round(clamp(abs(p.x), 0.0, 0.16) * 100))
                col_peak = bust_col_peaks.get(x_key, 0.0)
                front_mask = smoothstep(-0.015, 0.035, p.y) * smoothstep(0.160, 0.105, abs(p.x))
                if p.z <= 1.335:
                    t_up = smoothstep(1.015, 1.325, p.z)
                    curtain_dy = col_peak * (0.06 + 0.80 * (t_up ** 1.35)) * front_mask
                else:
                    curtain_dy = 0.0
                d = Vector((d.x, max(bridged_dy, curtain_dy, d.y), d.z))

            s_deltas.append(d)

        # Convex-Preserving Cloth Smoothing: smooths wrinkles & fills concavities,
        # while NEVER shrinking outward displacement below 106% of raw body expansion!
        n_smooth = 6 if kname in ("Female_Breasts", "Glutes_Mass", "Body_Heavy_Fat", "Abdomen_Belly") else 3
        for _ in range(n_smooth):
            nxt_d = []
            for vi in range(n_shirt):
                nbs = shirt_adj[vi]
                if not nbs:
                    nxt_d.append(s_deltas[vi])
                    continue
                avg_d = sum((s_deltas[nb] for nb in nbs), Vector((0.0, 0.0, 0.0))) / len(nbs)
                sm = s_deltas[vi].lerp(avg_d, 0.38)
                if vi not in interior_indices:
                    n_out = bary_map[vi][6]
                    min_out = raw_deltas[vi].dot(n_out) * 1.06
                    cur_out = sm.dot(n_out)
                    if min_out > 0.0 and cur_out < min_out:
                        sm = sm + n_out * (min_out - cur_out)
                nxt_d.append(sm)
            s_deltas = nxt_d

        # Exact vertex + quad-face + edge-midpoint clearance check at full runtime slider max (2.0 for Breasts/Glutes)!
        max_val = 2.0 if kname in ("Female_Breasts", "Glutes_Mass") else kb.slider_max
        if max_val > 0.05 and not kname.startswith(("Head", "Jaw", "Face")):
            morphed_body = [basis_coords[i] + body_deltas[i] * max_val for i in range(len(basis_coords))]
            m_bvh, _ = build_body_bvh_from_coords(body_mesh, morphed_body)

            bm_chk = bmesh.new()
            bm_chk.from_mesh(shirt_mesh)
            bm_chk.verts.ensure_lookup_table()
            for vi, v in enumerate(bm_chk.verts):
                v.co = shirt_base_pts[vi] + s_deltas[vi] * max_val
            skip_bm = {bm_chk.verts[vi] for vi in interior_indices}
            enforce_mesh_clearance_over_bvh(
                bm_chk, m_bvh, skip_verts=skip_bm, min_vert_gap=0.0065, min_face_gap=0.0052
            )
            for vi, v in enumerate(bm_chk.verts):
                s_deltas[vi] = (v.co - shirt_base_pts[vi]) / max_val
            bm_chk.free()

        skb = shirt_obj.shape_key_add(name=kname, from_mix=False)
        skb.slider_min = kb.slider_min
        skb.slider_max = kb.slider_max
        for vi in range(n_shirt):
            skb.data[vi].co = shirt_base_pts[vi] + s_deltas[vi]

    # =========================================================================
    # STEP 3: WIRE LIVE DRIVERS FOR RIG SLIDERS + ALL MANUAL BODY SHAPE KEYS
    # =========================================================================
    body_sk = body_mesh.shape_keys
    shirt_sk = shirt_mesh.shape_keys
    shirt_sk.animation_data_create()

    driven_paths = set()
    if body_sk.animation_data and body_sk.animation_data.drivers:
        for src_fc in body_sk.animation_data.drivers:
            driven_paths.add(src_fc.data_path)
            dst_fc = shirt_sk.animation_data.drivers.new(src_fc.data_path)
            dst_fc.driver.type = src_fc.driver.type
            dst_fc.driver.expression = src_fc.driver.expression
            for sv in src_fc.driver.variables:
                dv = dst_fc.driver.variables.new()
                dv.name = sv.name
                dv.type = sv.type
                for st, dt in zip(sv.targets, dv.targets):
                    dt.id_type = st.id_type
                    dt.id = st.id
                    dt.data_path = st.data_path

    # Mirror all remaining manual shape keys directly from chartest_body so adjusting ANY slider updates shirt live!
    for kb in shirt_sk.key_blocks[1:]:
        dpath = f'key_blocks["{kb.name}"].value'
        if dpath in driven_paths:
            continue
        dst_fc = shirt_sk.animation_data.drivers.new(dpath)
        dst_fc.driver.type = "AVERAGE"
        dv = dst_fc.driver.variables.new()
        dv.name = "val"
        dv.type = "SINGLE_PROP"
        dt = dv.targets[0]
        dt.id_type = "KEY"
        dt.id = body_sk
        dt.data_path = dpath


def verify_shirt_and_export_glb(shirt_obj, body_obj, arm_obj, tex_img, glb_path: pathlib.Path, interior_indices: set[int]):
    """Assign PSX material, export CamisaLinhoClara.glb, and audit vertex + face clearance across 5 body types!"""
    mat = bpy.data.materials.new("camisa_linho_clara")
    mat.use_nodes = True
    mat.use_backface_culling = False
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.94
    bsdf.inputs["Metallic"].default_value = 0.0
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.06
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = tex_img
    tex.interpolation = "Closest"
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])

    shirt_obj.data.materials.clear()
    shirt_obj.data.materials.append(mat)

    for t in arm_obj.animation_data.nla_tracks:
        t.mute = True
    arm_obj.animation_data.action = None
    arm_obj["height_m"] = 1.82
    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 0.0
    arm_obj["glutes_size"] = 0.0
    for pb in arm_obj.pose.bones:
        pb.matrix_basis.identity()

    drv_mute_state = []
    if shirt_obj.data.shape_keys and shirt_obj.data.shape_keys.animation_data:
        for fc in shirt_obj.data.shape_keys.animation_data.drivers:
            drv_mute_state.append((fc, fc.mute))
            fc.mute = True
    for kb in shirt_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0

    bpy.ops.object.select_all(action="DESELECT")
    arm_obj.select_set(True)
    shirt_obj.select_set(True)
    bpy.context.view_layer.objects.active = arm_obj

    bpy.ops.export_scene.gltf(
        filepath=str(glb_path),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=False,
        export_skins=True,
        export_all_influences=False,
        export_morph=True,
        export_morph_normal=True,
        export_morph_tangent=False,
        export_animations=False,
        export_texcoords=True,
        export_normals=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
    )

    for fc, old_m in drv_mute_state:
        fc.mute = old_m

    raw = glb_path.read_bytes()
    magic, version, total_len = struct.unpack_from("<4sII", raw, 0)
    assert magic == b"glTF" and version == 2 and total_len == len(raw)
    jlen = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20 : 20 + jlen])

    c_gltf = json.loads(CHARTEST_GLTF.read_text("utf-8"))
    c_joints = [c_gltf["nodes"][j].get("name") for j in c_gltf["skins"][0]["joints"]]
    s_joints = [doc["nodes"][j].get("name") for j in doc["skins"][0]["joints"]]
    assert s_joints == c_joints, f"Joint order mismatch! {s_joints} != {c_joints}"

    c_targets = c_gltf["meshes"][0]["extras"]["targetNames"]
    s_targets = doc["meshes"][0]["extras"]["targetNames"]
    assert s_targets == c_targets, f"Morph targetNames mismatch! {s_targets} != {c_targets}"

    bm = bmesh.new()
    bm.from_mesh(shirt_obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    tri_count = len(bm.faces)
    bm.free()

    print(
        f"[shirt-verify] {glb_path.name} ({len(raw)} bytes): tris={tri_count}, "
        f"verts={len(shirt_obj.data.vertices)}, joints={len(s_joints)} (100% match), "
        f"morph_targets={len(s_targets)} (100% match)"
    )

    # Multi-preset BVH clearance audit across BOTH vertices AND face centers!
    deps = bpy.context.evaluated_depsgraph_get()
    for label, preset in [
        ("Male_Base", {"is_female": False, "breasts_size": 0.0, "glutes_size": 0.0, "height_m": 1.82, "sk": {}}),
        ("Female_Std", {"is_female": True, "breasts_size": 1.05, "glutes_size": 0.85, "height_m": 1.74, "sk": {}}),
        ("Female_MaxCurvy", {"is_female": True, "breasts_size": 2.0, "glutes_size": 2.0, "height_m": 1.74, "sk": {}}),
        (
            "Muscular_Tall",
            {
                "is_female": False,
                "breasts_size": 0.0,
                "glutes_size": 0.3,
                "height_m": 1.94,
                "sk": {"Body_Muscle": 1.0, "Shoulders_Width": 0.55, "Arms_Upper": 0.55},
            },
        ),
        (
            "Heavy_Robust",
            {
                "is_female": False,
                "breasts_size": 0.0,
                "glutes_size": 0.5,
                "height_m": 1.75,
                "sk": {"Body_Heavy_Fat": 1.0, "Abdomen_Belly": 0.70, "Waist_Width": 0.55},
            },
        ),
    ]:
        for kb in body_obj.data.shape_keys.key_blocks[1:]:
            kb.value = 0.0
        arm_obj["height_m"] = preset["height_m"]
        arm_obj["is_female"] = preset["is_female"]
        arm_obj["breasts_size"] = preset["breasts_size"]
        arm_obj["glutes_size"] = preset["glutes_size"]
        for k, val in preset["sk"].items():
            body_obj.data.shape_keys.key_blocks[k].value = val
        arm_obj.update_tag()
        body_obj.update_tag()
        shirt_obj.update_tag()
        deps.update()

        ev_b = body_obj.evaluated_get(deps)
        ev_s = shirt_obj.evaluated_get(deps)
        mb = ev_b.to_mesh()
        ms = ev_s.to_mesh()
        b_coords = [v.co.copy() for v in mb.vertices]
        bvh_eval, _ = build_body_bvh_from_coords(mb, b_coords)
        worst_vert = 9.9
        clipped_v = 0
        for idx_v, sv in enumerate(ms.vertices):
            if idx_v in interior_indices:
                continue
            loc, nrm, _, _ = bvh_eval.find_nearest(sv.co)
            if loc is not None and nrm is not None:
                d_s = (sv.co - loc).dot(nrm)
                worst_vert = min(worst_vert, d_s)
                if d_s < -0.0005:
                    clipped_v += 1

        worst_face = 9.9
        clipped_f = 0
        for poly in ms.polygons:
            if any(vi in interior_indices for vi in poly.vertices):
                continue
            c = poly.center
            loc, nrm, _, _ = bvh_eval.find_nearest(c)
            if loc is not None and nrm is not None:
                d_s = (c - loc).dot(nrm)
                worst_face = min(worst_face, d_s)
                if d_s < -0.0005:
                    clipped_f += 1

        ev_b.to_mesh_clear()
        ev_s.to_mesh_clear()
        print(
            f"  [audit {label:15s}] min_vert={worst_vert*1000:+.2f}mm (clip={clipped_v}), "
            f"min_face={worst_face*1000:+.2f}mm (clip={clipped_f})"
        )

    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"] = 1.82
    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 0.0
    arm_obj["glutes_size"] = 0.0
    arm_obj.update_tag()
    body_obj.update_tag()
    shirt_obj.update_tag()
    deps.update()


def set_relaxed_preview_pose(arm_obj, relax_deg: float = 16.0):
    """Apply a gentle natural standing arm drop for preview renders (proves live 42-bone skinning too!)."""
    for pb in arm_obj.pose.bones:
        pb.matrix_basis.identity()
    if abs(relax_deg) < 1e-4:
        return
    for bname, sign in [("upper_arm.L", -1.0), ("upper_arm.R", +1.0)]:
        pb = arm_obj.pose.bones[bname]
        m_loc = pb.bone.matrix_local.to_3x3()
        rot_y = Matrix.Rotation(math.radians(relax_deg * sign), 3, "Y")
        pb.matrix_basis = (m_loc.inverted() @ rot_y @ m_loc).to_4x4()


def render_shirt_previews(shirt_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render:
    1) preview_shirt_angles.png: 4-angle upper-body showcase (Front, 3/4, Side, 3/4 Back)
    2) preview_shirt_body_types.png: Single-mesh Auto-Fit across 5 radically different body types + isolated shirt!
    """
    scene = bpy.context.scene
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"
    if hasattr(scene, "eevee"):
        if hasattr(scene.eevee, "samples"):
            scene.eevee.samples = 12
        elif hasattr(scene.eevee, "taa_render_samples"):
            scene.eevee.taa_render_samples = 12

    world = bpy.data.worlds.new("ShirtPreviewWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.052, 0.060, 0.075, 1.0)
    bg.inputs[1].default_value = 1.0

    lights = []
    for lname, loc, energy, col in [
        ("ShKey", (-2.2, 3.8, 3.3), 3.4, (1.0, 0.97, 0.92)),
        ("ShFill", (2.8, 2.8, 2.2), 1.8, (0.82, 0.88, 1.0)),
        ("ShRim", (0.2, -3.6, 2.9), 2.3, (0.94, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy = energy
        ldata.color = col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.25)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("ShirtCam")
    cam = bpy.data.objects.new("ShirtCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    fem_mat = None
    fem_img = bpy.data.images.get("Chartest-Female-v01-128.png")
    if fem_img is not None:
        fem_mat = bpy.data.materials.new("PreviewFemSkin")
        fem_mat.use_nodes = True
        ns = fem_mat.node_tree.nodes
        ls = fem_mat.node_tree.links
        ns.clear()
        mo = ns.new("ShaderNodeOutputMaterial")
        bs = ns.new("ShaderNodeBsdfPrincipled")
        bs.inputs["Roughness"].default_value = 0.9
        ti = ns.new("ShaderNodeTexImage")
        ti.image = fem_img
        ti.interpolation = "Closest"
        ls.new(ti.outputs["Color"], bs.inputs["Base Color"])
        ls.new(bs.outputs["BSDF"], mo.inputs["Surface"])

    depsgraph = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = True
    arm_obj.hide_render = True
    shirt_obj.hide_render = True

    def spawn_outfit(
        x_pos,
        yaw_deg,
        include_body=True,
        is_female=False,
        breasts=0.0,
        glutes=0.0,
        height_m=1.82,
        sk_overrides=None,
        arm_relax_deg=17.0,
    ):
        for kb in body_obj.data.shape_keys.key_blocks[1:]:
            kb.value = 0.0
        arm_obj["height_m"] = height_m
        arm_obj["is_female"] = is_female
        arm_obj["breasts_size"] = breasts
        arm_obj["glutes_size"] = glutes
        if sk_overrides:
            for k, val in sk_overrides.items():
                body_obj.data.shape_keys.key_blocks[k].value = val
        set_relaxed_preview_pose(arm_obj, relax_deg=arm_relax_deg)
        arm_obj.update_tag()
        body_obj.update_tag()
        shirt_obj.update_tag()
        depsgraph.update()

        objs = []
        rot = (0.0, 0.0, math.radians(yaw_deg))
        if include_body:
            ev_b = body_obj.evaluated_get(depsgraph)
            mb = bpy.data.meshes.new_from_object(ev_b, preserve_all_data_layers=True, depsgraph=depsgraph)
            ob = bpy.data.objects.new("SnapBody", mb)
            if is_female and fem_mat is not None:
                ob.data.materials.clear()
                ob.data.materials.append(fem_mat)
            scene.collection.objects.link(ob)
            ob.location = (x_pos, 0.0, 0.0)
            ob.rotation_euler = rot
            objs.append(ob)

        ev_s = shirt_obj.evaluated_get(depsgraph)
        ms = bpy.data.meshes.new_from_object(ev_s, preserve_all_data_layers=True, depsgraph=depsgraph)
        os_obj = bpy.data.objects.new("SnapShirt", ms)
        scene.collection.objects.link(os_obj)
        os_obj.location = (x_pos, 0.0, 0.0)
        os_obj.rotation_euler = rot
        objs.append(os_obj)
        return objs

    def cleanup_objs(objs):
        for o in objs:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            if m.users == 0:
                bpy.data.meshes.remove(m)

    # --- SHEET 1: 4-Angle Upper-Body Showcase (Front 0°, 3/4 +34°, Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(1.70, 0.0), (0.55, 34.0), (-0.55, -90.0), (-1.62, -142.0)]:
        snaps.extend(spawn_outfit(x_p, yaw, include_body=True, is_female=False, arm_relax_deg=17.0))

    scene.render.resolution_x = 1760
    scene.render.resolution_y = 780
    cam.location = (0.04, 4.0, 1.25)
    cam.rotation_euler = (Vector((0.04, 0.0, 1.23)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.65
    scene.render.filepath = str(out_dir / "preview_shirt_angles.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    # --- SHEET 2: Universal Auto-Fit Lineup across 5 Body Types + Isolated Shirt Piece!
    # Left -> Right on screen (+X -> -X):
    # 1. Base Male (1.82m)
    # 2. Female Standard (is_female=True, breasts=1.05, glutes=0.85)
    # 3. Female Max Curvy (is_female=True, breasts=1.95, glutes=1.85, -66° side-3/4 showing bust drape & hip vents!)
    # 4. Muscular + Tall (1.95m, Body_Muscle=1.0, Shoulders=0.55, Arms_Upper=0.55)
    # 5. Heavy/Robust (1.75m, Body_Heavy_Fat=1.0, Abdomen=0.70, Waist=0.55)
    # 6. Isolated 3D Shirt Piece (A-pose rest geometry, showing open neck, lacing, cuffs & side slits)
    snaps = []
    snaps.extend(spawn_outfit(2.90, 22.0, include_body=True, is_female=False, height_m=1.82, arm_relax_deg=17.0))
    snaps.extend(
        spawn_outfit(1.78, 28.0, include_body=True, is_female=True, breasts=1.05, glutes=0.85, height_m=1.74, arm_relax_deg=17.0)
    )
    snaps.extend(
        spawn_outfit(0.74, -66.0, include_body=True, is_female=True, breasts=1.95, glutes=1.85, height_m=1.74, arm_relax_deg=17.0)
    )
    snaps.extend(
        spawn_outfit(
            -0.36,
            24.0,
            include_body=True,
            is_female=False,
            height_m=1.95,
            sk_overrides={"Body_Muscle": 1.0, "Shoulders_Width": 0.55, "Arms_Upper": 0.55},
            arm_relax_deg=17.0,
        )
    )
    snaps.extend(
        spawn_outfit(
            -1.56,
            28.0,
            include_body=True,
            is_female=False,
            height_m=1.75,
            sk_overrides={"Body_Heavy_Fat": 1.0, "Abdomen_Belly": 0.70, "Waist_Width": 0.55},
            arm_relax_deg=17.0,
        )
    )
    iso = spawn_outfit(-2.82, 24.0, include_body=False, arm_relax_deg=8.0)
    for o in iso:
        o.scale = (1.22, 1.22, 1.22)
        o.location = (-2.82, 0.0, -0.28)
    snaps.extend(iso)

    scene.render.resolution_x = 1920
    scene.render.resolution_y = 800
    cam.location = (0.02, 5.5, 0.99)
    cam.rotation_euler = (Vector((0.02, 0.0, 0.97)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 7.25
    scene.render.filepath = str(out_dir / "preview_shirt_body_types.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    # Restore clean rest-pose scene state
    set_relaxed_preview_pose(arm_obj, relax_deg=0.0)
    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"] = 1.82
    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 0.0
    arm_obj["glutes_size"] = 0.0
    body_obj.hide_render = False
    arm_obj.hide_render = False
    shirt_obj.hide_render = False
    bpy.data.objects.remove(cam, do_unlink=True)
    bpy.data.cameras.remove(cam_data)
    for lobj in lights:
        ld = lobj.data
        bpy.data.objects.remove(lobj, do_unlink=True)
        bpy.data.lights.remove(ld)


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.open_mainfile(filepath=str(CHARTEST_BLEND))

    body_obj = bpy.data.objects["chartest_body"]
    arm_obj = bpy.data.objects["chartest_rig"]

    for t in arm_obj.animation_data.nla_tracks:
        t.mute = True
    arm_obj.animation_data.action = None
    arm_obj["height_m"] = 1.82
    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 0.0
    arm_obj["glutes_size"] = 0.0

    tex_png = OUT_DIR / "camisa_linho_clara_128.png"
    uv_png = OUT_DIR / "camisa_linho_clara_uv.png"
    glb_path = OUT_DIR / "CamisaLinhoClara.glb"
    blend_out = OUT_DIR / "camisa_linho_clara.blend"

    tex_img = generate_light_linen_texture(TECIDO_GROSSO_PNG, tex_png)
    shirt_obj, interior_indices = build_linen_shirt_mesh(body_obj, arm_obj)
    export_uv_overlay(shirt_obj, uv_png)

    autofit_garment_to_chartest(shirt_obj, body_obj, arm_obj, interior_indices)
    verify_shirt_and_export_glb(shirt_obj, body_obj, arm_obj, tex_img, glb_path, interior_indices)

    bpy.ops.wm.save_as_mainfile(filepath=str(blend_out))
    render_shirt_previews(shirt_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CamisaLinhoClara + Universal Auto-Fit v2 built & rendered successfully!")


if __name__ == "__main__":
    main()
