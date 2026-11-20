"""Build Grimora low-poly PSX White Short-Sleeve Shirt (CamisaMangaCurtaBranca) using grimora_cloth_kit."""

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
    bell,
    clamp,
    clone_clean_body_bmesh,
    enforce_mesh_clearance_over_bvh,
    extrude_closed_rim_inward,
    finalize_garment_pipeline,
    open_clean_chartest,
    ordered_boundary_loop,
    smoothstep,
)

OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_short_sleeve_shirt"
)


def generate_white_woven_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive a 128x128 crisp PSX natural-white woven shirt texture from tecido_grosso.png."""
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
            sig = (lum[y][x] - s) * 1.0 + (s - 0.18) * 0.22
            sig_grid[y][x] = sig
            vals.append(sig)

    vs = sorted(vals)
    v_lo, v_hi = vs[int(0.03 * len(vs))], vs[int(0.97 * len(vs))]
    v_span = max(v_hi - v_lo, 1e-4)

    # Clean natural PSX soft-white / bleached linen palette (whiter than oatmeal CamisaLinhoClara, never neon)
    w_dark = (0.69, 0.68, 0.65)
    w_mid = (0.83, 0.82, 0.79)
    w_hi = (0.94, 0.93, 0.90)
    inner_col = (0.16, 0.15, 0.14)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((sig_grid[y][x] - v_lo) / v_span, 0.0, 1.0)
            if v < 0.08:
                f = 0.85 + 0.30 * tn
                out_px[idx : idx + 4] = [inner_col[0] * f, inner_col[1] * f, inner_col[2] * f, 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = w_dark[0] + (w_mid[0] - w_dark[0]) * k
                    g = w_dark[1] + (w_mid[1] - w_dark[1]) * k
                    b = w_dark[2] + (w_mid[2] - w_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = w_mid[0] + (w_hi[0] - w_mid[0]) * k
                    g = w_mid[1] + (w_hi[1] - w_mid[1]) * k
                    b = w_mid[2] + (w_hi[2] - w_mid[2]) * k

                # Subtle ribbed collar/cuff stitch band at y in (18..25)
                if 18 <= y <= 25:
                    rib = 0.95 + 0.05 * math.cos(x * math.pi)
                    r, g, b = r * rib, g * rib, b * rib
                if y in (25, 26) and ((x // 2) % 2 == 0):
                    r, g, b = r * 0.83, g * 0.83, b * 0.81
                wear = 1.0 - 0.05 * smoothstep(0.24, 0.14, v)
                out_px[idx : idx + 4] = [clamp(r * wear), clamp(g * wear), clamp(b * wear), 1.0]

    img = bpy.data.images.new("camisa_curta_branca_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_short_sleeve_shirt(body_obj, arm_obj, body_bvh, basis_coords):
    bm = clone_clean_body_bmesh(body_obj, basis_coords)
    ua_l = arm_obj.data.bones["upper_arm.L"]
    ua_r = arm_obj.data.bones["upper_arm.R"]

    def keep_face(f) -> bool:
        c = f.calc_center_median()
        if c.z < 1.015:
            return False
        r_neck = math.hypot(c.x, (c.y + 0.088) * 0.85)
        if c.z > 1.560 or (c.z > 1.522 and r_neck < 0.078):
            return False
        # Cut cleanly at mid-bicep (t_ua <= 0.67 between quad rings 0.61 and 0.74!) and drop all forearm/hand faces
        for ua in (ua_l, ua_r):
            u = (ua.tail_local - ua.head_local).normalized()
            L = (ua.tail_local - ua.head_local).length
            if abs(c.x) > 0.21 and (c - ua.head_local).dot(u) / L > 0.67:
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
    sleeve_l_loop = next(lp for lp in b_loops if sum(v.co.x for v in lp) / len(lp) < -0.20)
    sleeve_r_loop = next(lp for lp in b_loops if sum(v.co.x for v in lp) / len(lp) > +0.20)
    boundary_set = set(collar_loop) | set(waist_loop) | set(sleeve_l_loop) | set(sleeve_r_loop)

    # 1) Shape open rounded Crew/Scoop neckline (wider & lower in front than V-slit shirt, NO split!)
    for v in collar_loop:
        out_xy = Vector((v.co.x, v.co.y + 0.088, 0.0))
        if out_xy.length > 1e-5:
            out_xy.normalize()
        front_f = smoothstep(-0.15, -0.02, v.co.y)
        # Widen collar opening outward (+15mm front scoop, +9mm sides/back) and lower front to z=1.474m
        v.co.x += out_xy.x * (0.009 + 0.005 * front_f)
        v.co.y += out_xy.y * (0.008 + 0.008 * front_f)
        target_z = 1.522 * (1.0 - front_f) + 1.474 * front_f + 0.006 * bell(abs(v.co.x), 0.085, 0.045)
        v.co.z = v.co.z * 0.25 + target_z * 0.75

    for v in waist_loop:
        v.co.z = 1.015 - 0.003 * smoothstep(-0.05, 0.06, v.co.y)

    # Soft cloth relaxation on interior vertices
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
        arm_t = smoothstep(0.16, 0.28, abs(p.x))
        waist_t = smoothstep(1.32, 1.02, p.z) * (1.0 - arm_t)
        lumbar = bell(p.z, 1.14, 0.16) * smoothstep(-0.04, -0.14, p.y) * (1.0 - arm_t) * 0.0085
        sternum = bell(p.x, 0.0, 0.055) * bell(p.z, 1.33, 0.12) * smoothstep(0.0, 0.06, p.y) * 0.0055
        folds = (0.5 + 0.5 * math.cos(math.atan2(p.x, p.y + 0.05) * 6.0)) * 0.0022 * waist_t
        slack = 0.0095 + 0.0045 * waist_t + 0.0055 * arm_t + lumbar + sternum + folds
        if v in collar_loop:
            slack = 0.0080
        base_pt = loc if (loc is not None and (p - loc).dot(out_n) < 0.003) else p
        v.co = base_pt + out_n * slack

    interior_verts = set()
    border_faces = []
    shadow_faces = []

    # 2) Short Sleeves with Rolled/Turned-Up Bicep Cuff Band (t = 0.54 -> 0.63 along upper_arm)
    for s_loop, ua_bone in [(sleeve_l_loop, ua_l), (sleeve_r_loop, ua_r)]:
        a_pt, b_pt = ua_bone.head_local, ua_bone.tail_local
        u_ax = (b_pt - a_pt).normalized()
        L_ua = (b_pt - a_pt).length

        v0, v1 = s_loop[0], s_loop[1]
        sh_e = bm.edges.get((v0, v1))
        flip_s = False
        if sh_e and sh_e.link_faces:
            lps = list(sh_e.link_faces[0].loops)
            flip_s = any(lp.vert == v0 and lps[(i + 1) % len(lps)].vert == v1 for i, lp in enumerate(lps))

        def bicep_ring(t_val: float, slack_m: float):
            c_ax = a_pt + u_ax * (L_ua * t_val)
            pts, r_dirs = [], []
            for v in s_loop:
                rel = v.co - a_pt
                r_dir = (rel - u_ax * rel.dot(u_ax)).normalized()
                r_dirs.append(r_dir)
                hit, _, _, _ = body_bvh.ray_cast(c_ax, r_dir, 0.16)
                r_skin = (hit - c_ax).length if hit is not None else 0.052
                pts.append(c_ax + r_dir * (r_skin + slack_m))
            return pts, r_dirs

        # Align existing cut loop cleanly at t=0.53, then extrude turned-up cuff fold (t=0.56 -> 0.635)
        base_pts, rad_dirs = bicep_ring(0.53, 0.0115)
        for v, pt in zip(s_loop, base_pts):
            v.co = pt

        prev_ring = s_loop
        for st_t, slk, is_cuff in [(0.565, 0.0145, True), (0.635, 0.0138, True)]:
            c_pts, _ = bicep_ring(st_t, slk)
            new_ring = [bm.verts.new(pt) for pt in c_pts]
            n_s = len(s_loop)
            for i in range(n_s):
                j = (i + 1) % n_s
                fq = bm.faces.new(
                    (prev_ring[j], prev_ring[i], new_ring[i], new_ring[j])
                    if flip_s
                    else (prev_ring[i], prev_ring[j], new_ring[j], new_ring[i])
                )
                if is_cuff:
                    border_faces.append(fq)
            prev_ring = new_ring

        rf, sf, rv, dv = extrude_closed_rim_inward(
            bm, prev_ring, [-rd for rd in rad_dirs], rim_thick=0.0052, sink_depth=0.0080
        )
        border_faces.extend(rf)
        shadow_faces.extend(sf)
        interior_verts.update(rv)
        interior_verts.update(dv)

    # 3) Ribbed Round Crew Collar Band (flat-stitched round band hugging collarbone/neck, NO V-slit!)
    cv0, cv1 = collar_loop[0], collar_loop[1]
    ce0 = bm.edges.get((cv0, cv1))
    flip_col = False
    if ce0 and ce0.link_faces:
        lps = list(ce0.link_faces[0].loops)
        flip_col = any(lp.vert == cv0 and lps[(i + 1) % len(lps)].vert == cv1 for i, lp in enumerate(lps))

    inner_collar_ring = []
    collar_in_dirs = []
    for v in collar_loop:
        in_xy = Vector((-v.co.x, -(v.co.y + 0.088), 0.0))
        if in_xy.length > 1e-5:
            in_xy.normalize()
        # Ribbed crew band steps 11mm inward toward neck & +5mm up along collarbone slope
        pt_c = v.co + in_xy * 0.0105 + Vector((0.0, 0.0, 0.0055))
        loc_c, nrm_c, _, _ = body_bvh.find_nearest(pt_c)
        if loc_c is not None and nrm_c is not None and (pt_c - loc_c).dot(nrm_c) < 0.0075:
            pt_c = loc_c + nrm_c * 0.0075
        inner_collar_ring.append(bm.verts.new(pt_c))
        collar_in_dirs.append((in_xy * 0.75 + Vector((0.0, 0.0, -0.65))).normalized())

    n_c = len(collar_loop)
    for i in range(n_c):
        j = (i + 1) % n_c
        fq = bm.faces.new(
            (collar_loop[j], collar_loop[i], inner_collar_ring[i], inner_collar_ring[j])
            if flip_col
            else (collar_loop[i], collar_loop[j], inner_collar_ring[j], inner_collar_ring[i])
        )
        border_faces.append(fq)

    crf, csf, crv, cdv = extrude_closed_rim_inward(
        bm, inner_collar_ring, collar_in_dirs, rim_thick=0.0040, sink_depth=0.0055
    )
    border_faces.extend(crf)
    shadow_faces.extend(csf)
    interior_verts.update(crv)
    interior_verts.update(cdv)

    # 4) Casual Straight Shirt Hem (z = 1.015 -> 0.968 -> 0.922m, clean closed untucked casual hem!)
    wv0, wv1 = waist_loop[0], waist_loop[1]
    we0 = bm.edges.get((wv0, wv1))
    flip_w = False
    if we0 and we0.link_faces:
        lps = list(we0.link_faces[0].loops)
        flip_w = any(lp.vert == wv0 and lps[(i + 1) % len(lps)].vert == wv1 for i, lp in enumerate(lps))

    prev_w = waist_loop
    for z_lvl, flare, is_hem in [(0.968, 0.0135, False), (0.922, 0.0155, True)]:
        cur_w = []
        for v in waist_loop:
            ang = math.atan2(v.co.x, v.co.y + 0.045)
            horiz = Vector((math.sin(ang), math.cos(ang), 0.0)).normalized()
            orig = Vector((0.0, -0.045, z_lvl))
            hit, _, _, _ = body_bvh.ray_cast(orig, horiz, 0.35)
            r_b = (hit - orig).length if hit is not None else math.hypot(v.co.x, v.co.y + 0.045)
            pleat = math.cos(ang * 6.0) * 0.0024
            pt = orig + horiz * max(r_b + flare + pleat, math.hypot(v.co.x, v.co.y + 0.045) + 0.002)
            pt.z = z_lvl - 0.004 * bell(pt.x, 0.0, 0.14)
            cur_w.append(bm.verts.new(pt))
        n_w = len(waist_loop)
        for i in range(n_w):
            j = (i + 1) % n_w
            fq = bm.faces.new(
                (prev_w[j], prev_w[i], cur_w[i], cur_w[j])
                if flip_w
                else (prev_w[i], prev_w[j], cur_w[j], cur_w[i])
            )
            if is_hem:
                border_faces.append(fq)
        prev_w = cur_w

    hem_in_dirs = [Vector((-v.co.x, -(v.co.y + 0.045), 0.12)).normalized() for v in prev_w]
    hrf, hsf, hrv, hdv = extrude_closed_rim_inward(bm, prev_w, hem_in_dirs, rim_thick=0.0046, sink_depth=0.0070)
    border_faces.extend(hrf)
    shadow_faces.extend(hsf)
    interior_verts.update(hrv)
    interior_verts.update(hdv)

    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0072, min_face_gap=0.0058
    )
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # 5) Clean Cylindrical Sewing-Pattern UVs
    uv_lay = bm.loops.layers.uv.new("UVMap")
    shd_set, brd_set = set(shadow_faces), set(border_faces)
    for f in bm.faces:
        if f in shd_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.035)
            continue
        if f in brd_set:
            c = f.calc_center_median()
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            for lp, u_val, p in u_raws:
                is_edge = (p.z < c.z) if abs(c.x) < 0.22 else (abs(p.x) > abs(c.x))
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.148 if is_edge else 0.205)
            continue

        c = f.calc_center_median()
        if abs(c.x) > 0.195 and c.z > 1.15:
            ua_b = ua_l if c.x < 0.0 else ua_r
            arm_dir = (ua_b.tail_local - ua_b.head_local).normalized()
            arm_len = (ua_b.tail_local - ua_b.head_local).length
            side_ref = arm_dir.cross(Vector((0.0, 0.0, 1.0))).normalized()
            up_ortho = side_ref.cross(arm_dir).normalized()
            u_list = []
            for lp in f.loops:
                rel = lp.vert.co - ua_b.head_local
                along = rel.dot(arm_dir) / max(arm_len, 1e-4)
                rad = rel - arm_dir * rel.dot(arm_dir)
                u_raw = (math.atan2(rad.dot(side_ref), rad.dot(up_ortho)) + math.pi) / (2 * math.pi)
                u_list.append((lp, u_raw, clamp(0.95 - along * 0.35, 0.55, 0.97)))
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
                v_val = clamp(0.22 + (p.z - 0.915) / (1.535 - 0.915) * 0.74, 0.21, 0.98)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.48:
                u_list = [(lp, u + 0.90 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("CamisaMangaCurtaBranca")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("CamisaMangaCurtaBranca", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_interior


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, body_bvh, basis_coords = open_clean_chartest()
    tex_img = generate_white_woven_texture(TECIDO_GROSSO_PNG, OUT_DIR / "camisa_curta_branca_128.png")
    shirt_obj, interior_idx = build_short_sleeve_shirt(body_obj, arm_obj, body_bvh, basis_coords)
    finalize_garment_pipeline(
        garment_obj=shirt_obj,
        body_obj=body_obj,
        arm_obj=arm_obj,
        tex_img=tex_img,
        interior_indices=interior_idx,
        out_dir=OUT_DIR,
        glb_name="CamisaMangaCurtaBranca.glb",
        uv_name="camisa_curta_branca_uv.png",
        blend_name="camisa_manga_curta_branca.blend",
    )
    print("[done] CamisaMangaCurtaBranca built, audited & rendered in 1 pass!")


if __name__ == "__main__":
    main()
