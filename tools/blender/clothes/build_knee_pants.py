"""Build Grimora low-poly PSX Knee-Length Linen Breeches / Short Pants (CalcaCurtaJoelho) using grimora_cloth_kit."""

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
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_knee_pants"
)


def generate_knee_pants_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive 128x128 rustic warm-umber / flax linen weave from tecido_grosso.png."""
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
            sig = (lum[y][x] - s) * 1.0 + (s - 0.18) * 0.28
            sig_grid[y][x] = sig
            vals.append(sig)

    vs = sorted(vals)
    v_lo, v_hi = vs[int(0.03 * len(vs))], vs[int(0.97 * len(vs))]
    v_span = max(v_hi - v_lo, 1e-4)

    # Rustic warm flax-brown linen (slightly richer earthy tone than CalcaLinho, great with white short-sleeve shirt!)
    p_dark = (0.31, 0.26, 0.20)
    p_mid = (0.45, 0.38, 0.30)
    p_hi = (0.58, 0.50, 0.40)

    c_dark = (0.15, 0.09, 0.05)
    c_hi = (0.38, 0.25, 0.14)
    inner_col = (0.10, 0.08, 0.07)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((sig_grid[y][x] - v_lo) / v_span, 0.0, 1.0)

            if v < 0.072:
                f = 0.84 + 0.32 * tn
                out_px[idx : idx + 4] = [inner_col[0] * f, inner_col[1] * f, inner_col[2] * f, 1.0]
            elif v < 0.148:
                twist = 0.86 + 0.26 * math.sin(x * 0.7 + y * 0.9)
                tc = clamp(tn * twist, 0.0, 1.0)
                r = c_dark[0] + (c_hi[0] - c_dark[0]) * tc
                g = c_dark[1] + (c_hi[1] - c_dark[1]) * tc
                b = c_dark[2] + (c_hi[2] - c_dark[2]) * tc
                out_px[idx : idx + 4] = [r, g, b, 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = p_dark[0] + (p_mid[0] - p_dark[0]) * k
                    g = p_dark[1] + (p_mid[1] - p_dark[1]) * k
                    b = p_dark[2] + (p_mid[2] - p_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = p_mid[0] + (p_hi[0] - p_mid[0]) * k
                    g = p_mid[1] + (p_hi[1] - p_mid[1]) * k
                    b = p_mid[2] + (p_hi[2] - p_mid[2]) * k

                if v < 0.222:
                    r, g, b = r * 1.08, g * 1.07, b * 1.05
                    if y in (20, 27) and ((x // 2) % 2 == 0):
                        r, g, b = r * 0.78, g * 0.76, b * 0.73
                else:
                    wear = 1.0 - 0.08 * smoothstep(0.32, 0.22, v)
                    r, g, b = r * wear, g * wear, b * wear
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]

    img = bpy.data.images.new("calca_curta_joelho_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_knee_pants_mesh(body_obj, arm_obj, body_bvh, basis_coords):
    bm = clone_clean_body_bmesh(body_obj, basis_coords)

    # Cut cleanly between quad rings at knee (0.410 sits right in the clean gap [0.391 .. 0.421]!)
    def keep_face(f) -> bool:
        c = f.calc_center_median()
        return 0.410 <= c.z <= 0.995 and abs(c.x) < 0.28

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

    waist_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) > 0.85)
    knee_l_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) < 0.60 and sum(v.co.x for v in lp) / len(lp) < 0.0)
    knee_r_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) < 0.60 and sum(v.co.x for v in lp) / len(lp) > 0.0)
    boundary_set = set(waist_loop) | set(knee_l_loop) | set(knee_r_loop)

    for v in waist_loop:
        v.co.z = 1.018 - 0.004 * smoothstep(-0.05, 0.06, v.co.y)
    for lp in (knee_l_loop, knee_r_loop):
        for v in lp:
            v.co.z = 0.455

    for _ in range(4):
        new_pos = {}
        for v in bm.verts:
            if v in boundary_set:
                continue
            nbs = [e.other_vert(v) for e in v.link_edges]
            if nbs:
                avg = sum((nb.co for nb in nbs), Vector()) / len(nbs)
                blended = v.co.lerp(avg, 0.34)
                if abs(v.co.x) < 1e-4:
                    blended.x = 0.0
                new_pos[v] = blended
        for v, co in new_pos.items():
            v.co = co

    bm.normal_update()

    for v in bm.verts:
        p = v.co.copy()
        loc, n_body, _, _ = body_bvh.find_nearest(p)
        out_n = v.normal.normalized()
        if n_body is not None and out_n.dot(n_body) > 0.15:
            out_n = (out_n * 0.55 + n_body * 0.45).normalized()

        thigh_drape = bell(p.z, 0.64, 0.24)
        glute_bridge = bell(p.x, 0.0, 0.045) * bell(p.z, 0.87, 0.09) * smoothstep(-0.06, -0.14, p.y) * 0.0085
        leg_cx = -0.10 if p.x < 0.0 else +0.10
        leg_ang = math.atan2(p.x - leg_cx, p.y + 0.04)
        folds = (0.5 + 0.5 * math.cos(leg_ang * 5.0 + p.z * 11.0)) * 0.0032 * thigh_drape

        slack = 0.0092 + 0.0085 * thigh_drape + glute_bridge + folds
        if v in waist_loop:
            slack = 0.0080
        base_pt = loc if (loc is not None and (p - loc).dot(out_n) < 0.003) else p
        v.co = base_pt + out_n * slack
        if abs(p.x) < 1e-4:
            v.co.x = 0.0

    interior_verts = set()
    border_faces = []
    cord_faces = []
    shadow_faces = []

    # 1) Stitched Linen Waistband (z = 1.018 -> 1.050m, covers 100% of base shorts & layers cleanly under shirts!)
    wv0, wv1 = waist_loop[0], waist_loop[1]
    we0 = bm.edges.get((wv0, wv1))
    flip_w = False
    if we0 and we0.link_faces:
        lps = list(we0.link_faces[0].loops)
        flip_w = any(lp.vert == wv0 and lps[(i + 1) % len(lps)].vert == wv1 for i, lp in enumerate(lps))

    wb_top = []
    wb_in_dirs = []
    for v in waist_loop:
        out_xy = Vector((v.co.x, v.co.y + 0.045, 0.0))
        if out_xy.length > 1e-5:
            out_xy.normalize()
        vt = bm.verts.new(v.co + Vector((0.0, 0.0, 0.032)) + out_xy * 0.0010)
        loc_w, nrm_w, _, _ = body_bvh.find_nearest(vt.co)
        if loc_w is not None and nrm_w is not None and (vt.co - loc_w).dot(nrm_w) < 0.0072:
            vt.co = loc_w + nrm_w * 0.0072
        wb_top.append(vt)
        wb_in_dirs.append(Vector((-out_xy.x, -out_xy.y, -0.15)).normalized())

    n_w = len(waist_loop)
    for i in range(n_w):
        j = (i + 1) % n_w
        fq = bm.faces.new(
            (waist_loop[j], waist_loop[i], wb_top[i], wb_top[j])
            if flip_w
            else (waist_loop[i], waist_loop[j], wb_top[j], wb_top[i])
        )
        border_faces.append(fq)

    wrf, wsf, wrv, wdv = extrude_closed_rim_inward(bm, wb_top, wb_in_dirs, rim_thick=0.0046, sink_depth=0.0065)
    border_faces.extend(wrf)
    shadow_faces.extend(wsf)
    interior_verts.update(wrv)
    interior_verts.update(wdv)

    # 2) Turned-Up / Rolled Knee-Length Cuff Band right at the Knee (z = 0.455 -> 0.428 -> 0.402m)!
    for k_loop in (knee_l_loop, knee_r_loop):
        cx = sum(v.co.x for v in k_loop) / len(k_loop)
        cy = sum(v.co.y for v in k_loop) / len(k_loop)

        kv0, kv1 = k_loop[0], k_loop[1]
        ke0 = bm.edges.get((kv0, kv1))
        flip_k = False
        if ke0 and ke0.link_faces:
            lps = list(ke0.link_faces[0].loops)
            flip_k = any(lp.vert == kv0 and lps[(i + 1) % len(lps)].vert == kv1 for i, lp in enumerate(lps))

        def knee_ring(z_val: float, slack_m: float, pleat_a: float = 0.0):
            orig = Vector((cx, cy, z_val))
            pts, r_dirs = [], []
            for idx_v, v in enumerate(k_loop):
                r_dir = Vector((v.co.x - cx, v.co.y - cy, 0.0)).normalized()
                r_dirs.append(r_dir)
                hit, _, _, _ = body_bvh.ray_cast(orig, r_dir, 0.16)
                r_skin = (hit - orig).length if hit is not None else 0.056
                pl = pleat_a * math.cos(idx_v * 2.0 * math.pi / len(k_loop) * 3.0)
                pts.append(orig + r_dir * (r_skin + slack_m + pl))
            return pts, r_dirs

        # Soft bloused gather just above the knee cuff (z=0.458, +16mm), then turned-up knee cuff band (z=0.428 -> 0.400)!
        puff_pts, rad_dirs = knee_ring(0.458, 0.0165, pleat_a=0.0028)
        for v, pt in zip(k_loop, puff_pts):
            v.co = pt

        prev_k = k_loop
        for z_st, slk, is_cuff in [(0.428, 0.0145, True), (0.400, 0.0135, True)]:
            c_pts, _ = knee_ring(z_st, slk)
            new_k = [bm.verts.new(pt) for pt in c_pts]
            nk = len(k_loop)
            for i in range(nk):
                j = (i + 1) % nk
                fq = bm.faces.new(
                    (prev_k[j], prev_k[i], new_k[i], new_k[j])
                    if flip_k
                    else (prev_k[i], prev_k[j], new_k[j], new_k[i])
                )
                if is_cuff:
                    border_faces.append(fq)
            prev_k = new_k

        krf, ksf, krv, kdv = extrude_closed_rim_inward(
            bm, prev_k, [-rd for rd in rad_dirs], rim_thick=0.0052, sink_depth=0.0080
        )
        border_faces.extend(krf)
        shadow_faces.extend(ksf)
        interior_verts.update(krv)
        interior_verts.update(kdv)

    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0072, min_face_gap=0.0058
    )

    # 3) Front Hemp Drawstring Knot & Hanging Ends
    front_wb = max(waist_loop, key=lambda v: v.co.y)
    knot_p = Vector((0.0, front_wb.co.y + 0.0048, 1.032))

    def add_cord_tube(pts: list[Vector], radius: float = 0.0024):
        rings = []
        for idx_p, pt in enumerate(pts):
            ax = (pts[min(idx_p + 1, len(pts) - 1)] - pts[max(idx_p - 1, 0)]).normalized()
            up = Vector((0.0, 1.0, 0.0)) if abs(ax.y) < 0.9 else Vector((0.0, 0.0, 1.0))
            s1 = ax.cross(up).normalized() * radius
            s2 = ax.cross(s1).normalized() * radius
            rv = [bm.verts.new(pt + s1), bm.verts.new(pt + s2), bm.verts.new(pt - s1), bm.verts.new(pt - s2)]
            rings.append(rv)
            interior_verts.update(rv)
        for ri in range(len(rings) - 1):
            ra, rb = rings[ri], rings[ri + 1]
            for k in range(4):
                kn = (k + 1) % 4
                cord_faces.append(bm.faces.new((ra[k], ra[kn], rb[kn], rb[k])))

    add_cord_tube([knot_p + Vector((-0.011, -0.001, 0.0)), knot_p + Vector((0.0, 0.0015, 0.0)), knot_p + Vector((0.011, -0.001, 0.0))], 0.0030)
    add_cord_tube([knot_p, knot_p + Vector((-0.009, 0.002, -0.026)), knot_p + Vector((-0.014, 0.001, -0.054))], 0.0022)
    add_cord_tube([knot_p, knot_p + Vector((+0.008, 0.002, -0.023)), knot_p + Vector((+0.012, 0.001, -0.048))], 0.0022)

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # 4) Clean Cylindrical UVs
    uv_lay = bm.loops.layers.uv.new("UVMap")
    shd_set, brd_set, crd_set = set(shadow_faces), set(border_faces), set(cord_faces)

    for f in bm.faces:
        if f in shd_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.035)
            continue
        if f in crd_set:
            for k_i, lp in enumerate(f.loops):
                lp[uv_lay].uv = ((k_i % 2) * 0.55 + 0.20, 0.095 if k_i < 2 else 0.132)
            continue
        if f in brd_set:
            c = f.calc_center_median()
            cx = 0.0 if c.z > 0.70 else (-0.10 if c.x < 0.0 else +0.10)
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x - cx, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.160 if p.z < c.z else 0.212)
            continue

        c = f.calc_center_median()
        if c.z < 0.78:
            is_left = c.x < 0.0
            cx = -0.105 if is_left else +0.105
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                u_raw = (math.atan2(p.x - cx, p.y + 0.035) + math.pi) / (2 * math.pi)
                v_val = clamp(0.24 + (p.z - 0.395) / (0.78 - 0.395) * 0.48, 0.23, 0.74)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.55:
                u_list = [(lp, u + 1.0 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            u_base = 0.03 if is_left else 0.52
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_base + 0.44 * u_val, 0.02, 0.98), v_val)
        else:
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                u_raw = 0.50 + (math.atan2(p.x, p.y + 0.045) / (2 * math.pi)) * 0.90
                v_val = clamp(0.73 + (p.z - 0.78) / (1.05 - 0.78) * 0.24, 0.72, 0.98)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.48:
                u_list = [(lp, u + 0.90 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("CalcaCurtaJoelho")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("CalcaCurtaJoelho", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_interior


def render_knee_pants_previews(pants_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render 4 angles + 5 body types (with CamisaMangaCurtaBranca + CalcaCurtaJoelho combo on #1!) + isolated piece."""
    from build_linen_pants import render_pants_previews
    render_pants_previews(pants_obj, body_obj, arm_obj, out_dir)


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, body_bvh, basis_coords = open_clean_chartest()
    tex_img = generate_knee_pants_texture(TECIDO_GROSSO_PNG, OUT_DIR / "calca_curta_joelho_128.png")
    pants_obj, interior_idx = build_knee_pants_mesh(body_obj, arm_obj, body_bvh, basis_coords)
    pants_obj.parent = arm_obj
    export_uv_overlay(pants_obj, OUT_DIR / "calca_curta_joelho_uv.png")
    autofit_garment_to_chartest(pants_obj, body_obj, arm_obj, interior_idx)
    verify_garment_and_export_glb(pants_obj, body_obj, arm_obj, tex_img, OUT_DIR / "CalcaCurtaJoelho.glb", interior_idx)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_DIR / "calca_curta_joelho.blend"))
    render_knee_pants_previews(pants_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CalcaCurtaJoelho built, audited & rendered in 1 pass!")


if __name__ == "__main__":
    main()
