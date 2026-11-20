"""Build Grimora low-poly PSX Long Medieval Tunic (TunicaLonga) using grimora_cloth_kit.

Features:
- High stood-up medieval split mandarin collar with antique ochre-gold woven border
- Wide draped long sleeves with ochre-embroidered cuffs
- 3D dark leather waist belt + low-poly aged-brass buckle at center-front
- Calf-length A-line tunic skirt (z = 1.015m down to z = 0.36m) with deep lateral walking slits (z = 0.78 -> 0.36m)
- Weathered forest-moss / dark olive-sage 128x128 woven texture derived from tecido_grosso.png
- Full 42-bone skinning + 24 convex-preserving Shape Keys (including Glutes_Mass back-skirt curtain bridge!)
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
    build_body_bvh_from_coords,
    clamp,
    clone_clean_body_bmesh,
    enforce_mesh_clearance_over_bvh,
    export_uv_overlay,
    extrude_closed_rim_inward,
    open_clean_chartest,
    ordered_boundary_loop,
    render_garment_previews,
    smoothstep,
    verify_garment_and_export_glb,
)

OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_long_tunic"
)


def generate_long_tunic_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive 128x128 4-zone PSX dark-fantasy atlas from tecido_grosso.png:
    - V in [0.34 .. 1.00]: Deep weathered forest-moss / dark ranger-olive woven cloth
    - V in [0.21 .. 0.33]: Antique ochre-gold embroidered geometric trim border (collar, cuffs, skirt hem, buckle)
    - V in [0.08 .. 0.20]: Dark oiled leather waist belt
    - V in [0.00 .. 0.075]: Deep interior cloth shadow lip
    """
    src = bpy.data.images.load(str(src_png), check_existing=True)
    w, h = src.size
    src_px = list(src.pixels[:])
    out_px = [0.0] * (w * h * 4)

    lum = [[0.0] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            i = (y * w + x) * 4
            lum[y][x] = 0.299 * src_px[i] + 0.587 * src_px[i + 1] + 0.114 * src_px[i + 2]

    sig_grid = [[0.0] * w for _ in range(h)]
    vals = []
    for y in range(h):
        for x in range(w):
            s = sum(lum[(y + dy) % h][(x + dx) % w] for dy in range(-3, 4) for dx in range(-3, 4)) / 49.0
            sig = (lum[y][x] - s) * 1.0 + (s - 0.18) * 0.26
            sig_grid[y][x] = sig
            vals.append(sig)

    vs = sorted(vals)
    v_lo, v_hi = vs[int(0.03 * len(vs))], vs[int(0.97 * len(vs))]
    v_span = max(v_hi - v_lo, 1e-4)

    # Deep forest-moss / weathered olive-slate wool-linen
    t_dark = (0.14, 0.19, 0.15)
    t_mid = (0.24, 0.31, 0.24)
    t_hi = (0.35, 0.43, 0.34)

    # Antique ochre / aged brass trim & buckle
    g_dark = (0.42, 0.31, 0.14)
    g_hi = (0.76, 0.61, 0.32)

    # Dark oiled leather belt
    l_dark = (0.11, 0.07, 0.05)
    l_hi = (0.25, 0.16, 0.10)

    inner_col = (0.08, 0.10, 0.08)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((sig_grid[y][x] - v_lo) / v_span, 0.0, 1.0)

            if v < 0.075:
                f = 0.84 + 0.32 * tn
                out_px[idx : idx + 4] = [inner_col[0] * f, inner_col[1] * f, inner_col[2] * f, 1.0]
            elif v < 0.205:
                grain = 0.92 + 0.12 * math.sin(x * 0.4 + y * 1.3)
                tl = clamp(tn * grain, 0.0, 1.0)
                r = l_dark[0] + (l_hi[0] - l_dark[0]) * tl
                g = l_dark[1] + (l_hi[1] - l_dark[1]) * tl
                b = l_dark[2] + (l_hi[2] - l_dark[2]) * tl
                if y in (11, 24) and ((x // 2) % 2 == 0):
                    r, g, b = r * 1.35, g * 1.30, b * 1.20
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            elif v < 0.335:
                # Woven geometric medieval trim pattern on moss backing
                is_motif = ((x + (y - 27)) % 4 in (0, 1)) or y in (28, 40)
                if is_motif:
                    tg = clamp(tn * 0.8 + 0.2, 0.0, 1.0)
                    r = g_dark[0] + (g_hi[0] - g_dark[0]) * tg
                    g = g_dark[1] + (g_hi[1] - g_dark[1]) * tg
                    b = g_dark[2] + (g_hi[2] - g_dark[2]) * tg
                else:
                    r, g, b = t_dark[0] * (0.85 + 0.4 * tn), t_dark[1] * (0.85 + 0.4 * tn), t_dark[2] * (0.85 + 0.4 * tn)
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = t_dark[0] + (t_mid[0] - t_dark[0]) * k
                    g = t_dark[1] + (t_mid[1] - t_dark[1]) * k
                    b = t_dark[2] + (t_mid[2] - t_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = t_mid[0] + (t_hi[0] - t_mid[0]) * k
                    g = t_mid[1] + (t_hi[1] - t_mid[1]) * k
                    b = t_mid[2] + (t_hi[2] - t_mid[2]) * k
                # Subtle hem weathering near lower skirt V=0.34..0.46
                hem_dirt = 1.0 - 0.12 * smoothstep(0.46, 0.34, v)
                out_px[idx : idx + 4] = [clamp(r * hem_dirt), clamp(g * hem_dirt), clamp(b * hem_dirt), 1.0]

    img = bpy.data.images.new("tunica_longa_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_long_tunic_mesh(body_obj, arm_obj, body_bvh, basis_coords):
    bm = clone_clean_body_bmesh(body_obj, basis_coords)
    fa_l = arm_obj.data.bones["forearm.L"]
    fa_r = arm_obj.data.bones["forearm.R"]
    ua_l = arm_obj.data.bones["upper_arm.L"]
    ua_r = arm_obj.data.bones["upper_arm.R"]

    def keep_face(f) -> bool:
        c = f.calc_center_median()
        if c.z < 1.015:
            return False
        r_neck = math.hypot(c.x, (c.y + 0.088) * 0.85)
        if c.z > 1.560 or (c.z > 1.522 and r_neck < 0.078):
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

    visited = set()
    b_loops = []
    for e in [ed for ed in bm.edges if ed.is_boundary]:
        if e in visited:
            continue
        comp, stack = [], [e]
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

    for v in collar_loop:
        front_f = smoothstep(-0.16, -0.03, v.co.y)
        v.co.z = v.co.z * 0.30 + (1.532 * (1.0 - front_f) + 1.504 * front_f) * 0.70

    for v in waist_loop:
        v.co.z = 1.015

    for _ in range(3):
        new_pos = {}
        for v in bm.verts:
            if v in boundary_set or abs(v.co.x) < 1e-4:
                continue
            nbs = [e.other_vert(v) for e in v.link_edges]
            if nbs:
                new_pos[v] = v.co.lerp(sum((nb.co for nb in nbs), Vector()) / len(nbs), 0.30)
        for v, co in new_pos.items():
            v.co = co

    bm.normal_update()
    for v in bm.verts:
        p = v.co.copy()
        loc, n_body, _, _ = body_bvh.find_nearest(p)
        out_n = v.normal.normalized()
        if n_body is not None and out_n.dot(n_body) > 0.15:
            out_n = (out_n * 0.55 + n_body * 0.45).normalized()
        arm_t = smoothstep(0.16, 0.30, abs(p.x))
        blouse_above_belt = bell(p.z, 1.13, 0.08) * (1.0 - arm_t) * 0.0055
        lumbar = bell(p.z, 1.14, 0.16) * smoothstep(-0.04, -0.14, p.y) * (1.0 - arm_t) * 0.0085
        folds = (0.5 + 0.5 * math.cos(math.atan2(p.x, p.y + 0.05) * 7.0)) * 0.0025 * (1.0 - arm_t)
        slack = 0.0098 + 0.0065 * arm_t + blouse_above_belt + lumbar + folds
        if v in collar_loop:
            slack = 0.0078
        base_pt = loc if (loc is not None and (p - loc).dot(out_n) < 0.003) else p
        v.co = base_pt + out_n * slack

    interior_verts = set()
    trim_faces = []
    belt_faces = []
    buckle_faces = []
    shadow_faces = []

    # 1) Wide Traveler/Mage Flared Long Sleeves with Embroidered Ochre Cuffs
    for w_loop, fa_bone in [(wrist_l_loop, fa_l), (wrist_r_loop, fa_r)]:
        a_pt, b_pt = fa_bone.head_local, fa_bone.tail_local
        u_ax = (b_pt - a_pt).normalized()
        L_fa = (b_pt - a_pt).length

        v0, v1 = w_loop[0], w_loop[1]
        sh_e = bm.edges.get((v0, v1))
        flip_w = False
        if sh_e and sh_e.link_faces:
            lps = list(sh_e.link_faces[0].loops)
            flip_w = any(lp.vert == v0 and lps[(i + 1) % len(lps)].vert == v1 for i, lp in enumerate(lps))

        def sleeve_ring(t_val: float, slack_m: float, drop_z: float = 0.0):
            c_ax = a_pt + u_ax * (L_fa * t_val)
            pts, r_dirs = [], []
            for v in w_loop:
                rel = v.co - a_pt
                r_dir = (rel - u_ax * rel.dot(u_ax)).normalized()
                r_dirs.append(r_dir)
                hit, _, _, _ = body_bvh.ray_cast(c_ax, r_dir, 0.16)
                r_skin = (hit - c_ax).length if hit is not None else 0.036
                # Gravity pull on underside (-Z) creates a classic wide medieval traveler/mage sleeve!
                under_bell = smoothstep(0.0, -0.85, r_dir.z) * drop_z
                pts.append(c_ax + r_dir * (r_skin + slack_m) + Vector((0.0, 0.0, -under_bell)))
            return pts, r_dirs

        mid_pts, rad_dirs = sleeve_ring(0.75, 0.0150, drop_z=0.010)
        for v, pt in zip(w_loop, mid_pts):
            v.co = pt

        prev_ring = w_loop
        for st_t, slk, drp, is_trim in [(0.84, 0.0185, 0.016, False), (0.915, 0.0205, 0.020, True)]:
            r_pts, _ = sleeve_ring(st_t, slk, drop_z=drp)
            new_ring = [bm.verts.new(pt) for pt in r_pts]
            n_w = len(w_loop)
            for i in range(n_w):
                j = (i + 1) % n_w
                fq = bm.faces.new(
                    (prev_ring[j], prev_ring[i], new_ring[i], new_ring[j])
                    if flip_w
                    else (prev_ring[i], prev_ring[j], new_ring[j], new_ring[i])
                )
                if is_trim:
                    trim_faces.append(fq)
            prev_ring = new_ring

        rf, sf, rv, dv = extrude_closed_rim_inward(
            bm, prev_ring, [-rd for rd in rad_dirs], rim_thick=0.0055, sink_depth=0.0090
        )
        trim_faces.extend(rf)
        shadow_faces.extend(sf)
        interior_verts.update(rv)
        interior_verts.update(dv)

    # 2) Calf-Length A-Line Tunic Skirt (z = 1.015m down to z = 0.360m) with Deep Side Slits!
    wv0, wv1 = waist_loop[0], waist_loop[1]
    we0 = bm.edges.get((wv0, wv1))
    flip_waist = False
    if we0 and we0.link_faces:
        lps = list(we0.link_faces[0].loops)
        flip_waist = any(lp.vert == wv0 and lps[(i + 1) % len(lps)].vert == wv1 for i, lp in enumerate(lps))

    n_waist = len(waist_loop)
    idx_left_vent = min(range(n_waist), key=lambda i: waist_loop[i].co.x)
    idx_right_vent = max(range(n_waist), key=lambda i: waist_loop[i].co.x)

    # Pre-measure max body envelope (xmax, ymin_back, ymax_front) per Z slice so skirt clears both legs cleanly!
    def skirt_point_for_angle(ang: float, z_lvl: float, extra_flare: float, pleat_amp: float) -> Vector:
        # Measure actual torso/leg cross-section near z_lvl (strictly exclude A-pose hands/arms at |x| > 0.26!)
        band = [p for p in basis_coords if abs(p.z - z_lvl) < 0.048 and abs(p.x) < 0.26]
        xmax_b = max((abs(p.x) for p in band), default=0.195)
        ymax_b = max((p.y for p in band), default=0.065)
        ymin_b = min((p.y for p in band), default=-0.155)

        y_center = 0.5 * (ymax_b + ymin_b)
        rx = xmax_b + extra_flare
        ry_front = (ymax_b - y_center) + extra_flare + 0.006
        ry_back = (y_center - ymin_b) + extra_flare + 0.008

        sx = math.sin(ang)
        cy = math.cos(ang)
        pleat = math.cos(ang * 8.0) * pleat_amp
        px = sx * (rx + pleat)
        py = y_center + cy * ((ry_front if cy >= 0.0 else ry_back) + pleat)

        # Also check raycast from (0, y_center, z_lvl) for upper pelvis (z > 0.80, max_dist=0.25 to ignore hands!)
        if z_lvl > 0.78:
            horiz = Vector((px, py - y_center, 0.0))
            if horiz.length > 1e-5:
                hd = horiz.normalized()
                orig = Vector((0.0, y_center, z_lvl))
                hit, _, _, _ = body_bvh.ray_cast(orig, hd, 0.25)
                if hit is not None and (hit - orig).length + extra_flare > horiz.length:
                    pt_r = orig + hd * ((hit - orig).length + extra_flare + pleat)
                    px, py = pt_r.x, pt_r.y
        return Vector((px, py, z_lvl))

    # Closed upper hip skirt rings: z = 0.945, z = 0.865
    prev_closed = waist_loop
    for z_c, flr in [(0.945, 0.0155), (0.865, 0.0185)]:
        cur_closed = []
        for v in waist_loop:
            ang = math.atan2(v.co.x, v.co.y + 0.045)
            cur_closed.append(bm.verts.new(skirt_point_for_angle(ang, z_c, flr, 0.0028)))
        for i in range(n_waist):
            j = (i + 1) % n_waist
            bm.faces.new(
                (prev_closed[j], prev_closed[i], cur_closed[i], cur_closed[j])
                if flip_waist
                else (prev_closed[i], prev_closed[j], cur_closed[j], cur_closed[i])
            )
        prev_closed = cur_closed

    # Below z = 0.865m, split at Left & Right hips into Front & Back long tunic panels down to z = 0.365m!
    def panel_seq(i_s, i_e):
        seq = [i_s]
        c_i = i_s
        while c_i != i_e:
            c_i = (c_i + 1) % n_waist
            seq.append(c_i)
        return seq

    panel_a = panel_seq(idx_left_vent, idx_right_vent)
    panel_b = panel_seq(idx_right_vent, idx_left_vent)

    lower_tiers = [
        (0.775, 0.022, 0.0034, False),
        (0.675, 0.027, 0.0042, False),
        (0.570, 0.033, 0.0050, False),
        (0.465, 0.040, 0.0058, False),
        (0.395, 0.046, 0.0064, False),
        (0.360, 0.049, 0.0066, True),
    ]

    for panel in (panel_a, panel_b):
        mean_y = sum(prev_closed[i].co.y for i in panel[1:-1]) / max(len(panel) - 2, 1)
        y_sign = +1.0 if mean_y >= -0.04 else -1.0
        prev_row = [prev_closed[i] for i in panel]

        for t_idx, (z_lvl, flr, pl_a, is_trim) in enumerate(lower_tiers, start=1):
            vent_gap = 0.0068 * t_idx  # Opens graceful ~4cm side walking slits toward calf!
            cur_row = []
            for p_i, w_i in enumerate(panel):
                ang = math.atan2(waist_loop[w_i].co.x, waist_loop[w_i].co.y + 0.045)
                pt = skirt_point_for_angle(ang, z_lvl, flr, pl_a)
                # Slight regal center dip on front/back tabard panels + side slit parting
                pt.z -= 0.010 * bell(pt.x, 0.0, 0.16)
                if p_i == 0 or p_i == len(panel) - 1:
                    pt.y += y_sign * vent_gap
                    pt.z += 0.004 * t_idx
                    # Add trim border along the vertical side-slit edges too? Even cleaner: bottom tier is full gold trim!
                cur_row.append(bm.verts.new(pt))

            for k in range(len(panel) - 1):
                fq = bm.faces.new(
                    (prev_row[k + 1], prev_row[k], cur_row[k], cur_row[k + 1])
                    if flip_waist
                    else (prev_row[k], prev_row[k + 1], cur_row[k + 1], cur_row[k])
                )
                if is_trim:
                    trim_faces.append(fq)
            prev_row = cur_row

    bm.edges.ensure_lookup_table()
    skirt_bnd_edges = [e for e in bm.edges if e.is_boundary and all(v.co.z < 0.90 for v in e.verts)]
    skirt_bnd_loop = ordered_boundary_loop(skirt_bnd_edges)
    skirt_in_dirs = [Vector((-v.co.x, -(v.co.y + 0.045), 0.10)).normalized() for v in skirt_bnd_loop]
    hrf, hsf, hrv, hdv = extrude_closed_rim_inward(
        bm, skirt_bnd_loop, skirt_in_dirs, rim_thick=0.0052, sink_depth=0.0080
    )
    trim_faces.extend(hrf)
    shadow_faces.extend(hsf)
    interior_verts.update(hrv)
    interior_verts.update(hdv)

    # 3) High Stood-Up Split Mandarin Collar with Ochre Embroidered Trim!
    front_center_vs = sorted(
        [v for v in bm.verts if abs(v.co.x) < 0.0015 and v.co.y > -0.055 and 1.430 <= v.co.z <= 1.545],
        key=lambda v: -v.co.z,
    )
    if len(front_center_vs) >= 2:
        # Narrow keyhole split on top 2 front vertices (18mm -> 8mm -> apex)
        for idx_s, v_orig in enumerate(front_center_vs[:-1]):
            sp = 0.011 if idx_s == 0 else 0.006
            pos_faces = [f for f in list(v_orig.link_faces) if f.calc_center_median().x > 0.0]
            v_r = bm.verts.new(v_orig.co.copy())
            for f in pos_faces:
                verts_f = [v_r if fv == v_orig else fv for fv in f.verts]
                bm.faces.remove(f)
                bm.faces.new(verts_f)
            v_orig.co.x, v_r.co.x = -sp, +sp
            v_orig.co.y += 0.0025
            v_r.co.y += 0.0025

    bm.edges.ensure_lookup_table()
    bm.faces.ensure_lookup_table()
    col_bnd = [e for e in bm.edges if e.is_boundary and all(v.co.z > 1.40 for v in e.verts)]
    col_loop = ordered_boundary_loop(col_bnd)

    cv0, cv1 = col_loop[0], col_loop[1]
    ce0 = bm.edges.get((cv0, cv1))
    flip_col = False
    if ce0 and ce0.link_faces:
        lps = list(ce0.link_faces[0].loops)
        flip_col = any(lp.vert == cv0 and lps[(i + 1) % len(lps)].vert == cv1 for i, lp in enumerate(lps))

    stand_vs = []
    col_in_dirs = []
    for v in col_loop:
        neck_ness = smoothstep(1.475, 1.510, v.co.z)
        out_xy = Vector((v.co.x, v.co.y + 0.092, 0.0))
        if out_xy.length > 1e-5:
            out_xy.normalize()
        # High mandarin collar stands +20mm around neck!
        vs = bm.verts.new(v.co + Vector((0.0, 0.0, 0.020 * neck_ness)) + out_xy * (0.0040 * neck_ness + 0.0018))
        loc_c, nrm_c, _, _ = body_bvh.find_nearest(vs.co)
        if loc_c is not None and nrm_c is not None and (vs.co - loc_c).dot(nrm_c) < 0.0075:
            vs.co = loc_c + nrm_c * 0.0075
        stand_vs.append(vs)
        if neck_ness > 0.35:
            col_in_dirs.append(Vector((-out_xy.x, -out_xy.y, -0.10)).normalized())
        else:
            sx = -1.0 if v.co.x < -0.002 else (+1.0 if v.co.x > 0.002 else 0.0)
            col_in_dirs.append(Vector((sx * 0.25, -0.95, -0.10)).normalized())

    n_col = len(col_loop)
    for i in range(n_col):
        j = (i + 1) % n_col
        fq = bm.faces.new(
            (col_loop[j], col_loop[i], stand_vs[i], stand_vs[j])
            if flip_col
            else (col_loop[i], col_loop[j], stand_vs[j], stand_vs[i])
        )
        trim_faces.append(fq)

    crf, csf, crv, cdv = extrude_closed_rim_inward(bm, stand_vs, col_in_dirs, rim_thick=0.0038, sink_depth=0.0050)
    trim_faces.extend(crf)
    shadow_faces.extend(csf)
    interior_verts.update(crv)
    interior_verts.update(cdv)

    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0072, min_face_gap=0.0058
    )

    # 4) 3D Dark Leather Waist Belt (z = 1.018 .. 1.082m) + Aged-Brass Center Buckle!
    bm_tmp = bm.copy()
    bmesh.ops.triangulate(bm_tmp, faces=bm_tmp.faces[:])
    tunic_bvh = BVHTree.FromBMesh(bm_tmp)
    bm_tmp.free()

    n_belt = 24
    belt_rows = []
    for z_b in (1.018, 1.082):
        r_vs = []
        for k in range(n_belt):
            ang = k * 2.0 * math.pi / n_belt
            horiz = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
            orig = Vector((0.0, -0.045, z_b))
            hit, _, _, _ = tunic_bvh.ray_cast(orig, horiz, 0.35)
            r_t = (hit - orig).length if hit is not None else 0.145
            r_vs.append(bm.verts.new(orig + horiz * (r_t + 0.0072)))
        belt_rows.append(r_vs)

    for k in range(n_belt):
        kn = (k + 1) % n_belt
        belt_faces.append(bm.faces.new((belt_rows[0][k], belt_rows[0][kn], belt_rows[1][kn], belt_rows[1][k])))
    for ri, z_dir in [(1, 1.0), (0, -1.0)]:
        # Solid top & bottom belt thickness lips
        in_ds = [Vector((-v.co.x, -(v.co.y + 0.045), 0.0)).normalized() for v in belt_rows[ri]]
        brf, bsf, brv, bdv = extrude_closed_rim_inward(bm, belt_rows[ri], in_ds, rim_thick=0.0038, sink_depth=0.0020)
        belt_faces.extend(brf)
        shadow_faces.extend(bsf)
        interior_verts.update(brv)
        interior_verts.update(bdv)

    # Front 3D Brass Belt Buckle at ang = 0 (+Y front)!
    y_bkl = belt_rows[0][0].co.y + 0.0045
    bx, bz0, bz1 = 0.024, 1.010, 1.090
    b_front = [
        bm.verts.new((-bx, y_bkl, bz0)),
        bm.verts.new((+bx, y_bkl, bz0)),
        bm.verts.new((+bx, y_bkl, bz1)),
        bm.verts.new((-bx, y_bkl, bz1)),
    ]
    b_back = [bm.verts.new((v.co.x, v.co.y - 0.006, v.co.z)) for v in b_front]
    interior_verts.update(b_back)
    buckle_faces.append(bm.faces.new(b_front))
    for k in range(4):
        kn = (k + 1) % 4
        buckle_faces.append(bm.faces.new((b_back[k], b_back[kn], b_front[kn], b_front[k])))

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # 5) Clean Cylindrical Sewing-Pattern UVs
    uv_lay = bm.loops.layers.uv.new("UVMap")
    shd_set = set(shadow_faces)
    trm_set = set(trim_faces) | set(buckle_faces)
    blt_set = set(belt_faces)

    for f in bm.faces:
        if f in shd_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.035)
            continue
        if f in blt_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.095 if p.z < c.z else 0.185)
            continue
        if f in trm_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                is_outer = (p.z < c.z) if abs(c.x) < 0.25 else (abs(p.x) > abs(c.x))
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.222 if is_outer else 0.318)
            continue

        c = f.calc_center_median()
        if abs(c.x) > 0.195 and c.z > 1.06:
            ua_b = ua_l if c.x < 0.0 else ua_r
            fa_b = fa_l if c.x < 0.0 else fa_r
            arm_vec = fa_b.tail_local - ua_b.head_local
            arm_dir, arm_len = arm_vec.normalized(), arm_vec.length
            side_ref = arm_dir.cross(Vector((0.0, 0.0, 1.0))).normalized()
            up_ortho = side_ref.cross(arm_dir).normalized()
            u_list = []
            for lp in f.loops:
                rel = lp.vert.co - ua_b.head_local
                along = rel.dot(arm_dir) / max(arm_len, 1e-4)
                rad = rel - arm_dir * rel.dot(arm_dir)
                u_raw = (math.atan2(rad.dot(side_ref), rad.dot(up_ortho)) + math.pi) / (2 * math.pi)
                u_list.append((lp, u_raw, clamp(0.96 - along * 0.56, 0.36, 0.98)))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.55:
                u_list = [(lp, u + 1.0 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            u_base = 0.04 if c.x < 0.0 else 0.54
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_base + 0.40 * u_val, 0.02, 0.98), v_val)
        else:
            # Split vertical UV cleanly into Upper Tunic (z >= 1.015) and Long Skirt (z < 1.015) to keep crisp ~140px/m texels!
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                u_raw = 0.50 + (math.atan2(p.x, p.y + 0.045) / (2 * math.pi)) * 0.90
                if c.z >= 1.015:
                    v_val = clamp(0.36 + (p.z - 1.015) / (1.545 - 1.015) * 0.61, 0.35, 0.98)
                else:
                    v_val = clamp(0.36 + (p.z - 0.355) / (1.015 - 0.355) * 0.61, 0.35, 0.98)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.48:
                u_list = [(lp, u + 0.90 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("TunicaLonga")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("TunicaLonga", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_interior


def smooth_glute_skirt_bridge(tunic_obj, body_obj, basis_coords, interior_indices: set[int]):
    """Refine Glutes_Mass on the long back skirt (z < 0.96, y < -0.04) so the tunic hangs in a regal
    continuous cloak-like curtain from the glutes down to the calves without dipping into the glute cleft!
    """
    skb = tunic_obj.data.shape_keys.key_blocks.get("Glutes_Mass")
    b_kb = body_obj.data.shape_keys.key_blocks.get("Glutes_Mass")
    if skb is None or b_kb is None:
        return

    # Measure peak backward (-Y) glute expansion on body
    peak_neg_dy = 0.0
    for i, bp in enumerate(basis_coords):
        if 0.78 <= bp.z <= 0.98 and bp.y < -0.05:
            dy = b_kb.data[i].co.y - bp.y  # negative when glutes grow backward (-Y)
            if dy < peak_neg_dy:
                peak_neg_dy = dy

    basis_t = tunic_obj.data.shape_keys.key_blocks[0].data
    for vi in range(len(tunic_obj.data.vertices)):
        if vi in interior_indices:
            continue
        p = basis_t[vi].co
        if p.z < 0.96 and p.y < -0.04:
            cur_delta = skb.data[vi].co - p
            # Curtain tapers smoothly from 92% of glute peak at z=0.86 down to 25% at calf z=0.36
            t_h = smoothstep(0.35, 0.86, p.z)
            back_mask = smoothstep(-0.03, -0.10, p.y) * smoothstep(0.22, 0.10, abs(p.x))
            curtain_dy = peak_neg_dy * (0.25 + 0.70 * (t_h ** 0.9)) * back_mask
            skb.data[vi].co = Vector((
                p.x + cur_delta.x,
                p.y + min(cur_delta.y, curtain_dy),
                p.z + cur_delta.z * 0.45,
            ))


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, body_bvh, basis_coords = open_clean_chartest()
    tex_img = generate_long_tunic_texture(TECIDO_GROSSO_PNG, OUT_DIR / "tunica_longa_128.png")
    tunic_obj, interior_idx = build_long_tunic_mesh(body_obj, arm_obj, body_bvh, basis_coords)
    tunic_obj.parent = arm_obj
    export_uv_overlay(tunic_obj, OUT_DIR / "tunica_longa_uv.png")
    autofit_garment_to_chartest(tunic_obj, body_obj, arm_obj, interior_idx)
    smooth_glute_skirt_bridge(tunic_obj, body_obj, basis_coords, interior_idx)
    verify_garment_and_export_glb(tunic_obj, body_obj, arm_obj, tex_img, OUT_DIR / "TunicaLonga.glb", interior_idx)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_DIR / "tunica_longa.blend"))
    render_garment_previews(tunic_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] TunicaLonga built, audited & rendered!")


if __name__ == "__main__":
    main()
