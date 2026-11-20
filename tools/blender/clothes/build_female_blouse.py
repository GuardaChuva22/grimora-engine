"""Build Grimora low-poly PSX Female Blouse (BlusaFeminina) using grimora_cloth_kit.

Distinct feminine medieval/dark-fantasy silhouette:
- Wide sweetheart/scoop neckline exposing collarbones, edged with antique cream ruffled trim
- 3/4 puffed sleeves gathered into cream-trimmed cuffs just below the elbow
- 3D cinched leather/fabric waist corset-sash with front crisscross cream lacing
- Flared wavy peplum hem with cream trim border
- Muted dusty-burgundy / rose-plum woven linen 128x128 palette derived from tecido_grosso.png
"""

import math
import pathlib
import sys

import bmesh
import bpy
from mathutils import Vector

SCRATCH_DIR = pathlib.Path(__file__).resolve().parent
if str(SCRATCH_DIR) not in sys.path:
    sys.path.insert(0, str(SCRATCH_DIR))

from grimora_cloth_kit import (
    TECIDO_GROSSO_PNG,
    autofit_garment_to_chartest,
    bell,
    clamp,
    clone_clean_body_bmesh,
    enforce_mesh_clearance_over_bvh,
    export_uv_overlay,
    extrude_closed_rim_inward,
    open_clean_chartest,
    ordered_boundary_loop,
    set_relaxed_preview_pose,
    smoothstep,
    verify_garment_and_export_glb,
)

OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_female_blouse"
)


def generate_blouse_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive 128x128 4-zone PSX atlas from tecido_grosso.png:
    - V in [0.34 .. 1.00]: Dusty burgundy / rose-plum woven blouse fabric
    - V in [0.21 .. 0.33]: Antique cream ruffled lace/trim border (collar, cuffs, peplum hem, lacing)
    - V in [0.08 .. 0.20]: Deep warm brown cinched waist corset-sash
    - V in [0.00 .. 0.075]: Dark interior shadow lip
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
            sig = (lum[y][x] - s) * 1.0 + (s - 0.18) * 0.25
            sig_grid[y][x] = sig
            vals.append(sig)

    vs = sorted(vals)
    v_lo, v_hi = vs[int(0.03 * len(vs))], vs[int(0.97 * len(vs))]
    v_span = max(v_hi - v_lo, 1e-4)

    # Dusty burgundy / antique rose-wine woven fabric
    b_dark = (0.34, 0.15, 0.18)
    b_mid = (0.52, 0.25, 0.28)
    b_hi = (0.67, 0.36, 0.39)

    # Antique cream ruffled trim
    c_dark = (0.66, 0.61, 0.52)
    c_hi = (0.91, 0.87, 0.78)

    # Warm dark brown corset-sash
    s_dark = (0.16, 0.10, 0.07)
    s_hi = (0.35, 0.22, 0.15)

    inner_col = (0.13, 0.09, 0.10)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((sig_grid[y][x] - v_lo) / v_span, 0.0, 1.0)

            if v < 0.075:
                f = 0.84 + 0.32 * tn
                out_px[idx : idx + 4] = [inner_col[0] * f, inner_col[1] * f, inner_col[2] * f, 1.0]
            elif v < 0.205:
                # Corset waist sash with subtle vertical ribbing & top/bottom stitch
                rib = 0.92 + 0.14 * math.cos(x * math.pi * 0.5)
                ts = clamp(tn * rib, 0.0, 1.0)
                r = s_dark[0] + (s_hi[0] - s_dark[0]) * ts
                g = s_dark[1] + (s_hi[1] - s_dark[1]) * ts
                b = s_dark[2] + (s_hi[2] - s_dark[2]) * ts
                if y in (11, 24) and ((x // 2) % 2 == 0):
                    r, g, b = r * 1.25, g * 1.22, b * 1.18
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            elif v < 0.335:
                # Cream ruffled trim + delicate scalloped eyelet dots
                ruffle = 0.90 + 0.16 * math.cos(x * math.pi * 0.66)
                tc = clamp(tn * 0.75 + 0.25, 0.0, 1.0) * ruffle
                r = clamp(c_dark[0] + (c_hi[0] - c_dark[0]) * tc)
                g = clamp(c_dark[1] + (c_hi[1] - c_dark[1]) * tc)
                b = clamp(c_dark[2] + (c_hi[2] - c_dark[2]) * tc)
                if y == 34 and (x % 4 == 1):
                    r, g, b = r * 0.72, g * 0.68, b * 0.65
                out_px[idx : idx + 4] = [r, g, b, 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = b_dark[0] + (b_mid[0] - b_dark[0]) * k
                    g = b_dark[1] + (b_mid[1] - b_dark[1]) * k
                    b = b_dark[2] + (b_mid[2] - b_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = b_mid[0] + (b_hi[0] - b_mid[0]) * k
                    g = b_mid[1] + (b_hi[1] - b_mid[1]) * k
                    b = b_mid[2] + (b_hi[2] - b_mid[2]) * k
                shade = 1.0 - 0.07 * smoothstep(0.42, 0.34, v)
                out_px[idx : idx + 4] = [clamp(r * shade), clamp(g * shade), clamp(b * shade), 1.0]

    img = bpy.data.images.new("blusa_feminina_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_female_blouse_mesh(body_obj, arm_obj, body_bvh, basis_coords):
    bm = clone_clean_body_bmesh(body_obj, basis_coords)
    fa_l = arm_obj.data.bones["forearm.L"]
    fa_r = arm_obj.data.bones["forearm.R"]
    ua_l = arm_obj.data.bones["upper_arm.L"]
    ua_r = arm_obj.data.bones["upper_arm.R"]

    def keep_face(f) -> bool:
        c = f.calc_center_median()
        if c.z < 1.015 or c.z > 1.545:
            return False
        # Wide open feminine collarbone neckline
        if c.z > 1.455 and math.hypot(c.x * 0.72, (c.y + 0.065) * 0.95) < 0.105:
            return False
        # Cut 3/4 sleeves right below the elbow (t_fa <= 0.38)
        for fa in (fa_l, fa_r):
            u = (fa.tail_local - fa.head_local).normalized()
            L = (fa.tail_local - fa.head_local).length
            if abs(c.x) > 0.32 and (c - fa.head_local).dot(u) / L > 0.38:
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

    collar_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) > 1.40)
    waist_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) < 1.08)
    cuff_l_loop = next(lp for lp in b_loops if sum(v.co.x for v in lp) / len(lp) < -0.25)
    cuff_r_loop = next(lp for lp in b_loops if sum(v.co.x for v in lp) / len(lp) > +0.25)
    boundary_set = set(collar_loop) | set(waist_loop) | set(cuff_l_loop) | set(cuff_r_loop)

    # 1) Sculpt Wide Sweetheart / Scoop Collarbone Neckline
    for v in collar_loop:
        front_f = smoothstep(-0.14, 0.01, v.co.y)
        sweetheart_dip = 0.018 * bell(v.co.x, 0.0, 0.055) * front_f
        shoulder_grace = 0.010 * bell(abs(v.co.x), 0.115, 0.055)
        target_z = 1.488 * (1.0 - front_f) + 1.434 * front_f - sweetheart_dip + shoulder_grace
        v.co.z = v.co.z * 0.25 + target_z * 0.75

    for v in waist_loop:
        v.co.z = 1.015 - 0.003 * smoothstep(-0.05, 0.06, v.co.y)

    # Relax interior into soft blouse fabric
    for _ in range(3):
        new_pos = {}
        for v in bm.verts:
            if v in boundary_set or abs(v.co.x) < 1e-4:
                continue
            nbs = [e.other_vert(v) for e in v.link_edges]
            if nbs:
                new_pos[v] = v.co.lerp(sum((nb.co for nb in nbs), Vector()) / len(nbs), 0.32)
        for v, co in new_pos.items():
            v.co = co

    bm.normal_update()

    # Airy puffed sleeves + cinched waist + soft bust blouse gather!
    for v in bm.verts:
        p = v.co.copy()
        loc, n_body, _, _ = body_bvh.find_nearest(p)
        out_n = v.normal.normalized()
        if n_body is not None and out_n.dot(n_body) > 0.15:
            out_n = (out_n * 0.50 + n_body * 0.50).normalized()

        arm_t = smoothstep(0.17, 0.27, abs(p.x))
        # Romantic bishop/lantern puff along upper arm & elbow!
        puff_bell = bell(abs(p.x), 0.32, 0.15) * arm_t
        sleeve_pleats = (0.5 + 0.5 * math.cos(p.y * 65.0 + p.z * 40.0)) * 0.0042 * arm_t

        # Snugger at cinched waist (z ~ 1.02..1.11), softly bloused right above corset sash (z ~ 1.15..1.25)
        blouse_over_sash = bell(p.z, 1.16, 0.075) * (1.0 - arm_t) * 0.0055
        lumbar = bell(p.z, 1.14, 0.15) * smoothstep(-0.04, -0.14, p.y) * (1.0 - arm_t) * 0.0075

        slack = 0.0088 + 0.0145 * puff_bell + sleeve_pleats + blouse_over_sash + lumbar
        if v in collar_loop:
            slack = 0.0078
        base_pt = loc if (loc is not None and (p - loc).dot(out_n) < 0.003) else p
        v.co = base_pt + out_n * slack

    interior_verts = set()
    cream_trim_faces = []
    sash_faces = []
    shadow_faces = []

    # 2) 3/4 Puffed Sleeves Gathered into Cream-Trimmed Elbow Cuffs
    for c_loop, fa_bone in [(cuff_l_loop, fa_l), (cuff_r_loop, fa_r)]:
        a_pt, b_pt = fa_bone.head_local, fa_bone.tail_local
        u_ax = (b_pt - a_pt).normalized()
        L_fa = (b_pt - a_pt).length

        v0, v1 = c_loop[0], c_loop[1]
        sh_e = bm.edges.get((v0, v1))
        flip_c = False
        if sh_e and sh_e.link_faces:
            lps = list(sh_e.link_faces[0].loops)
            flip_c = any(lp.vert == v0 and lps[(i + 1) % len(lps)].vert == v1 for i, lp in enumerate(lps))

        def elbow_ring(t_val: float, slack_m: float, pleat_amp: float = 0.0):
            c_ax = a_pt + u_ax * (L_fa * t_val)
            pts, r_dirs = [], []
            for idx_v, v in enumerate(c_loop):
                rel = v.co - a_pt
                r_dir = (rel - u_ax * rel.dot(u_ax)).normalized()
                r_dirs.append(r_dir)
                hit, _, _, _ = body_bvh.ray_cast(c_ax, r_dir, 0.16)
                r_skin = (hit - c_ax).length if hit is not None else 0.044
                pleat = pleat_amp * math.cos(idx_v * 2.0 * math.pi / len(c_loop) * 4.0)
                pts.append(c_ax + r_dir * (r_skin + slack_m + pleat))
            return pts, r_dirs

        # Airy lantern puff at t=0.25 (+20mm), gathering sharply into cream cuff band (t=0.34 -> 0.41) + subtle ruffle flare (t=0.45)!
        puff_pts, rad_dirs = elbow_ring(0.25, 0.0195, pleat_amp=0.0035)
        for v, pt in zip(c_loop, puff_pts):
            v.co = pt

        prev_ring = c_loop
        for st_t, slk, pl_a, is_trim in [
            (0.34, 0.0088, 0.0, False),
            (0.40, 0.0085, 0.0, True),
            (0.45, 0.0125, 0.0022, True),
        ]:
            r_pts, _ = elbow_ring(st_t, slk, pleat_amp=pl_a)
            new_ring = [bm.verts.new(pt) for pt in r_pts]
            n_c = len(c_loop)
            for i in range(n_c):
                j = (i + 1) % n_c
                fq = bm.faces.new(
                    (prev_ring[j], prev_ring[i], new_ring[i], new_ring[j])
                    if flip_c
                    else (prev_ring[i], prev_ring[j], new_ring[j], new_ring[i])
                )
                if is_trim:
                    cream_trim_faces.append(fq)
            prev_ring = new_ring

        rf, sf, rv, dv = extrude_closed_rim_inward(
            bm, prev_ring, [-rd for rd in rad_dirs], rim_thick=0.0045, sink_depth=0.0070
        )
        cream_trim_faces.extend(rf)
        shadow_faces.extend(sf)
        interior_verts.update(rv)
        interior_verts.update(dv)

    # 3) Delicate Cream Ruffled Sweetheart Collar Trim
    cv0, cv1 = collar_loop[0], collar_loop[1]
    ce0 = bm.edges.get((cv0, cv1))
    flip_col = False
    if ce0 and ce0.link_faces:
        lps = list(ce0.link_faces[0].loops)
        flip_col = any(lp.vert == cv0 and lps[(i + 1) % len(lps)].vert == cv1 for i, lp in enumerate(lps))

    trim_collar_ring = []
    collar_in_dirs = []
    for idx_v, v in enumerate(collar_loop):
        in_xy = Vector((-v.co.x, -(v.co.y + 0.070), 0.0))
        if in_xy.length > 1e-5:
            in_xy.normalize()
        wave = 0.0014 * math.cos(idx_v * 2.0 * math.pi / len(collar_loop) * 6.0)
        pt_c = v.co + in_xy * 0.0110 + Vector((0.0, 0.0, 0.0065 + wave))
        loc_c, nrm_c, _, _ = body_bvh.find_nearest(pt_c)
        if loc_c is not None and nrm_c is not None and (pt_c - loc_c).dot(nrm_c) < 0.0075:
            pt_c = loc_c + nrm_c * 0.0075
        trim_collar_ring.append(bm.verts.new(pt_c))
        collar_in_dirs.append((in_xy * 0.70 + Vector((0.0, 0.0, -0.70))).normalized())

    n_col = len(collar_loop)
    for i in range(n_col):
        j = (i + 1) % n_col
        fq = bm.faces.new(
            (collar_loop[j], collar_loop[i], trim_collar_ring[i], trim_collar_ring[j])
            if flip_col
            else (collar_loop[i], collar_loop[j], trim_collar_ring[j], trim_collar_ring[i])
        )
        cream_trim_faces.append(fq)

    crf, csf, crv, cdv = extrude_closed_rim_inward(
        bm, trim_collar_ring, collar_in_dirs, rim_thick=0.0038, sink_depth=0.0052
    )
    cream_trim_faces.extend(crf)
    shadow_faces.extend(csf)
    interior_verts.update(crv)
    interior_verts.update(cdv)

    # 4) Flared Wavy Peplum Ruffle Hem (z = 1.015 -> 0.962 -> 0.912m) with Cream Trim Border
    wv0, wv1 = waist_loop[0], waist_loop[1]
    we0 = bm.edges.get((wv0, wv1))
    flip_w = False
    if we0 and we0.link_faces:
        lps = list(we0.link_faces[0].loops)
        flip_w = any(lp.vert == wv0 and lps[(i + 1) % len(lps)].vert == wv1 for i, lp in enumerate(lps))

    prev_w = waist_loop
    for z_lvl, flare, wave_amp, is_trim in [
        (0.962, 0.0175, 0.0030, False),
        (0.922, 0.0255, 0.0052, False),
        (0.904, 0.0285, 0.0062, True),
    ]:
        cur_w = []
        for v in waist_loop:
            ang = math.atan2(v.co.x, v.co.y + 0.045)
            horiz = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
            orig = Vector((0.0, -0.045, z_lvl))
            hit, _, _, _ = body_bvh.ray_cast(orig, horiz, 0.35)
            r_b = (hit - orig).length if hit is not None else math.hypot(v.co.x, v.co.y + 0.045)
            ruffle = math.cos(ang * 9.0) * wave_amp
            pt = orig + horiz * max(r_b + flare + ruffle, math.hypot(v.co.x, v.co.y + 0.045) + 0.004)
            pt.z = z_lvl - 0.005 * bell(pt.x, 0.0, 0.14) + ruffle * 0.35
            cur_w.append(bm.verts.new(pt))
        n_w = len(waist_loop)
        for i in range(n_w):
            j = (i + 1) % n_w
            fq = bm.faces.new(
                (prev_w[j], prev_w[i], cur_w[i], cur_w[j])
                if flip_w
                else (prev_w[i], prev_w[j], cur_w[j], cur_w[i])
            )
            if is_trim:
                cream_trim_faces.append(fq)
        prev_w = cur_w

    hem_in_dirs = [Vector((-v.co.x, -(v.co.y + 0.045), 0.15)).normalized() for v in prev_w]
    hrf, hsf, hrv, hdv = extrude_closed_rim_inward(bm, prev_w, hem_in_dirs, rim_thick=0.0046, sink_depth=0.0068)
    cream_trim_faces.extend(hrf)
    shadow_faces.extend(hsf)
    interior_verts.update(hrv)
    interior_verts.update(hdv)

    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0072, min_face_gap=0.0058
    )

    # 5) 3D Cinched Waist Corset-Sash (z = 1.018 .. 1.112m) + Front Crisscross Cream Lacing!
    # Build BVH of the blouse itself so the corset sash sits cleanly +3.2mm proud over the blouse waist!
    bm_blouse_tmp = bm.copy()
    bmesh.ops.triangulate(bm_blouse_tmp, faces=bm_blouse_tmp.faces[:])
    blouse_bvh = bmesh.types.BMesh  # placeholder ref
    from mathutils.bvhtree import BVHTree as _BVH
    blouse_bvh = _BVH.FromBMesh(bm_blouse_tmp)
    bm_blouse_tmp.free()

    # Sweep 22 columns from front-right (+0.14 rad) around the back to front-left (2*pi - 0.14 rad) -> leaves crisp front corset gap!
    n_sash_cols = 23
    sash_z_rows = [1.018, 1.064, 1.110]
    sash_grid = []  # [row_idx][col_idx]
    for r_i, z_s in enumerate(sash_z_rows):
        row_vs = []
        for c_i in range(n_sash_cols):
            # ang = 0 is center-front (+Y); sweep from +0.135 rad (right placket) through pi (back) to 2*pi - 0.135 (left placket)
            frac = c_i / (n_sash_cols - 1)
            ang = 0.135 + frac * (2.0 * math.pi - 0.270)
            # Subtle V-point corset rise at center-front plackets & back!
            front_peak = 0.014 * bell(min(ang, 2.0 * math.pi - ang), 0.135, 0.45) * (1.0 if r_i == 2 else (-0.5 if r_i == 0 else 0.2))
            z_cur = z_s + front_peak
            horiz = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
            orig = Vector((0.0, -0.045, z_cur))
            hit, _, _, _ = blouse_bvh.ray_cast(orig, horiz, 0.35)
            r_bl = (hit - orig).length if hit is not None else 0.135
            v_s = bm.verts.new(orig + horiz * (r_bl + 0.0034))
            row_vs.append(v_s)
        sash_grid.append(row_vs)

    for r_i in range(2):
        for c_i in range(n_sash_cols - 1):
            fq = bm.faces.new(
                (sash_grid[r_i][c_i], sash_grid[r_i][c_i + 1], sash_grid[r_i + 1][c_i + 1], sash_grid[r_i + 1][c_i])
            )
            sash_faces.append(fq)

    # Add 2 crisscross cream corset laces across the front sash opening!
    def add_lace_cord(p_a: Vector, p_b: Vector, radius: float = 0.0020):
        ax = (p_b - p_a).normalized()
        up = Vector((0.0, 0.0, 1.0)) if abs(ax.z) < 0.9 else Vector((0.0, 1.0, 0.0))
        s1 = ax.cross(up).normalized() * radius
        s2 = ax.cross(s1).normalized() * radius
        p_m = (p_a + p_b) * 0.5 + Vector((0.0, 0.0025, 0.0))
        rings = []
        for pt in (p_a, p_m, p_b):
            rv = [bm.verts.new(pt + s1), bm.verts.new(pt + s2), bm.verts.new(pt - s1), bm.verts.new(pt - s2)]
            rings.append(rv)
            interior_verts.update(rv)
        for ri in range(2):
            ra, rb = rings[ri], rings[ri + 1]
            for k in range(4):
                kn = (k + 1) % 4
                cream_trim_faces.append(bm.faces.new((ra[k], ra[kn], rb[kn], rb[k])))

    r_bot, r_mid, r_top = sash_grid[0][0].co, sash_grid[1][0].co, sash_grid[2][0].co
    l_bot, l_mid, l_top = sash_grid[0][-1].co, sash_grid[1][-1].co, sash_grid[2][-1].co
    add_lace_cord(r_top.lerp(r_mid, 0.2), l_mid.lerp(l_bot, 0.15))
    add_lace_cord(l_top.lerp(l_mid, 0.2), r_mid.lerp(r_bot, 0.15))
    add_lace_cord(r_mid.lerp(r_top, 0.15), l_bot.lerp(l_mid, 0.2))
    add_lace_cord(l_mid.lerp(l_top, 0.15), r_bot.lerp(r_mid, 0.2))

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # 6) Assign Clean 4-Zone UVs
    uv_lay = bm.loops.layers.uv.new("UVMap")
    shd_set = set(shadow_faces)
    trim_set = set(cream_trim_faces)
    sash_set = set(sash_faces)

    for f in bm.faces:
        if f in shd_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.035)
            continue
        if f in sash_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                v_val = 0.095 if p.z < c.z else 0.185
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)
            continue
        if f in trim_set:
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            c = f.calc_center_median()
            for lp, u_val, p in u_raws:
                is_outer = (p.z < c.z) if abs(c.x) < 0.25 else (abs(p.x) > abs(c.x))
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.225 if is_outer else 0.315)
            continue

        c = f.calc_center_median()
        if abs(c.x) > 0.195 and c.z > 1.10:
            ua_b = ua_l if c.x < 0.0 else ua_r
            fa_b = fa_l if c.x < 0.0 else fa_r
            arm_vec = fa_b.tail_local - ua_b.head_local
            arm_dir = arm_vec.normalized()
            arm_len = arm_vec.length
            side_ref = arm_dir.cross(Vector((0.0, 0.0, 1.0))).normalized()
            up_ortho = side_ref.cross(arm_dir).normalized()
            u_list = []
            for lp in f.loops:
                rel = lp.vert.co - ua_b.head_local
                along = rel.dot(arm_dir) / max(arm_len, 1e-4)
                rad = rel - arm_dir * rel.dot(arm_dir)
                u_raw = (math.atan2(rad.dot(side_ref), rad.dot(up_ortho)) + math.pi) / (2 * math.pi)
                u_list.append((lp, u_raw, clamp(0.96 - along * 0.52, 0.38, 0.98)))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.55:
                u_list = [(lp, u + 1.0 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            u_base = 0.04 if c.x < 0.0 else 0.54
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_base + 0.40 * u_val, 0.02, 0.98), v_val)
        else:
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                u_raw = 0.50 + (math.atan2(p.x, p.y + 0.045) / (2 * math.pi)) * 0.90
                v_val = clamp(0.36 + (p.z - 0.905) / (1.515 - 0.905) * 0.61, 0.35, 0.98)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.48:
                u_list = [(lp, u + 0.90 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("BlusaFeminina")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("BlusaFeminina", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_interior


def render_female_blouse_previews(blouse_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render 4-angle showcase ON FEMALE BODY + 5-body-type universal Auto-Fit lineup!"""
    scene = bpy.context.scene
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"
    if hasattr(scene, "eevee"):
        if hasattr(scene.eevee, "samples"):
            scene.eevee.samples = 12
        elif hasattr(scene.eevee, "taa_render_samples"):
            scene.eevee.taa_render_samples = 12

    world = bpy.data.worlds.new("BlouseWorld")
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.052, 0.060, 0.075, 1.0)

    lights = []
    for lname, loc, energy, col in [
        ("BlKey", (-2.2, 3.8, 3.3), 3.4, (1.0, 0.97, 0.92)),
        ("BlFill", (2.8, 2.8, 2.2), 1.8, (0.82, 0.88, 1.0)),
        ("BlRim", (0.2, -3.6, 2.9), 2.3, (0.94, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy, ldata.color = energy, col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.25)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("BlCam")
    cam = bpy.data.objects.new("BlCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    fem_mat = None
    fem_img = bpy.data.images.get("Chartest-Female-v01-128.png")
    if fem_img is not None:
        fem_mat = bpy.data.materials.new("BlFemSkin")
        fem_mat.use_nodes = True
        ns, ls = fem_mat.node_tree.nodes, fem_mat.node_tree.links
        ns.clear()
        mo, bs, ti = ns.new("ShaderNodeOutputMaterial"), ns.new("ShaderNodeBsdfPrincipled"), ns.new("ShaderNodeTexImage")
        bs.inputs["Roughness"].default_value = 0.9
        ti.image, ti.interpolation = fem_img, "Closest"
        ls.new(ti.outputs["Color"], bs.inputs["Base Color"])
        ls.new(bs.outputs["BSDF"], mo.inputs["Surface"])

    deps = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = arm_obj.hide_render = blouse_obj.hide_render = True

    def spawn(x_p, yaw, include_body=True, is_female=True, breasts=1.15, glutes=0.95, height_m=1.74, sk=None, relax=17.0):
        for kb in body_obj.data.shape_keys.key_blocks[1:]:
            kb.value = 0.0
        arm_obj["height_m"] = height_m
        arm_obj["is_female"] = is_female
        arm_obj["breasts_size"] = breasts
        arm_obj["glutes_size"] = glutes
        if sk:
            for k, val in sk.items():
                body_obj.data.shape_keys.key_blocks[k].value = val
        set_relaxed_preview_pose(arm_obj, relax_deg=relax)
        arm_obj.update_tag()
        body_obj.update_tag()
        blouse_obj.update_tag()
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
        ev_s = blouse_obj.evaluated_get(deps)
        ms = bpy.data.meshes.new_from_object(ev_s, preserve_all_data_layers=True, depsgraph=deps)
        os_o = bpy.data.objects.new("SnapBlouse", ms)
        scene.collection.objects.link(os_o)
        os_o.location, os_o.rotation_euler = (x_p, 0.0, 0.0), rot
        objs.append(os_o)
        return objs

    def clean(objs):
        for o in objs:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            if m.users == 0:
                bpy.data.meshes.remove(m)

    # Sheet 1: 4 Angles on Female Body (Front 0°, 3/4 +34°, Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(1.68, 0.0), (0.54, 34.0), (-0.54, -90.0), (-1.60, -142.0)]:
        snaps.extend(spawn(x_p, yaw, include_body=True, is_female=True, breasts=1.15, glutes=0.95, height_m=1.74))
    scene.render.resolution_x, scene.render.resolution_y = 1760, 780
    cam.location = (0.04, 4.0, 1.20)
    cam.rotation_euler = (Vector((0.04, 0.0, 1.18)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.55
    scene.render.filepath = str(out_dir / "preview_blouse_angles.png")
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    # Sheet 2: 5 Body Types (Female Std, Female Max Curvy -62°, Female Slim Petite, Female Robust/Curvy, Male Base) + Isolated Piece!
    snaps = []
    snaps.extend(spawn(2.88, 24.0, is_female=True, breasts=1.10, glutes=0.90, height_m=1.74))
    snaps.extend(spawn(1.78, -62.0, is_female=True, breasts=1.95, glutes=1.85, height_m=1.74))
    snaps.extend(spawn(0.72, 26.0, is_female=True, breasts=0.45, glutes=0.35, height_m=1.60, sk={"Body_Gaunt_Lean": 0.65}))
    snaps.extend(
        spawn(-0.38, 26.0, is_female=True, breasts=1.55, glutes=1.40, height_m=1.72, sk={"Body_Heavy_Fat": 0.85, "Waist_Width": 0.35})
    )
    snaps.extend(spawn(-1.56, 24.0, is_female=False, breasts=0.0, glutes=0.0, height_m=1.84))
    iso = spawn(-2.80, 24.0, include_body=False, is_female=False, breasts=0.0, glutes=0.0, height_m=1.82, relax=8.0)
    for o in iso:
        o.scale = (1.24, 1.24, 1.24)
        o.location = (-2.80, 0.0, -0.30)
    snaps.extend(iso)

    scene.render.resolution_x, scene.render.resolution_y = 1920, 800
    cam.location = (0.02, 5.5, 0.96)
    cam.rotation_euler = (Vector((0.02, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 7.20
    scene.render.filepath = str(out_dir / "preview_blouse_body_types.png")
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    set_relaxed_preview_pose(arm_obj, relax_deg=0.0)
    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"], arm_obj["is_female"], arm_obj["breasts_size"], arm_obj["glutes_size"] = 1.82, False, 0.0, 0.0
    body_obj.hide_render = arm_obj.hide_render = blouse_obj.hide_render = False


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, body_bvh, basis_coords = open_clean_chartest()
    tex_img = generate_blouse_texture(TECIDO_GROSSO_PNG, OUT_DIR / "blusa_feminina_128.png")
    blouse_obj, interior_idx = build_female_blouse_mesh(body_obj, arm_obj, body_bvh, basis_coords)
    blouse_obj.parent = arm_obj
    export_uv_overlay(blouse_obj, OUT_DIR / "blusa_feminina_uv.png")
    autofit_garment_to_chartest(blouse_obj, body_obj, arm_obj, interior_idx)
    verify_garment_and_export_glb(blouse_obj, body_obj, arm_obj, tex_img, OUT_DIR / "BlusaFeminina.glb", interior_idx)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_DIR / "blusa_feminina.blend"))
    render_female_blouse_previews(blouse_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] BlusaFeminina built, audited & rendered!")


if __name__ == "__main__":
    main()
