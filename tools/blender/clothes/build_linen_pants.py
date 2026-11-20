"""Build Grimora low-poly PSX Linen Trousers (CalcaLinho) using grimora_cloth_kit.

Features:
- Warm raw-flax / earthy sand-taupe 128x128 linen weave derived from tecido_grosso.png (pairs naturally with CamisaLinhoClara!)
- Snug folded waistband (z ~ 1.005m, +7.5mm so untucked shirts layer cleanly over it without clipping!)
  + rustic front hemp drawstring knot & hanging cord ends
- Relaxed medieval linen leg drape over thighs & knees, tapering to turned-up ankle cuffs (z ~ 0.095m)
- Full 42-bone skinning + 24 convex-preserving Shape Keys (zero clipping across Male, Female, Max Curvy Glutes=2.0, Muscular, Heavy)
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
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_linen_pants"
)


def generate_linen_pants_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive 128x128 raw flax / earthy sand-taupe linen atlas from tecido_grosso.png:
    - V in [0.22 .. 1.00]: Raw flax / taupe-sand woven linen body + subtle ankle wear near V=0.22..0.30
    - V in [0.15 .. 0.22]: Stitched waistband & rolled ankle cuff trim
    - V in [0.075 .. 0.148]: Braided dark brown hemp drawstring cord
    - V in [0.00 .. 0.072]: Deep interior cloth shadow lip
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
            sig = (lum[y][x] - s) * 1.0 + (s - 0.18) * 0.28
            sig_grid[y][x] = sig
            vals.append(sig)

    vs = sorted(vals)
    v_lo, v_hi = vs[int(0.03 * len(vs))], vs[int(0.97 * len(vs))]
    v_span = max(v_hi - v_lo, 1e-4)

    # Raw unbleached flax / warm earthy taupe-sand linen (harmonious contrast with light cream CamisaLinhoClara!)
    p_dark = (0.35, 0.30, 0.24)
    p_mid = (0.49, 0.43, 0.35)
    p_hi = (0.62, 0.56, 0.46)

    c_dark = (0.16, 0.10, 0.06)
    c_hi = (0.40, 0.27, 0.16)
    inner_col = (0.11, 0.09, 0.08)

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

                # Slightly lighter rolled cuff / waistband band in V=[0.15..0.22] + stitch line
                if v < 0.222:
                    r, g, b = r * 1.06, g * 1.05, b * 1.04
                    if y in (20, 27) and ((x // 2) % 2 == 0):
                        r, g, b = r * 0.80, g * 0.78, b * 0.75
                else:
                    # Subtle road-dust/wear gradient near lower shin V=0.22..0.32
                    wear = 1.0 - 0.10 * smoothstep(0.32, 0.22, v)
                    r, g, b = r * wear, g * wear, b * wear
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]

    img = bpy.data.images.new("calca_linho_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_linen_pants_mesh(body_obj, arm_obj, body_bvh, basis_coords):
    bm = clone_clean_body_bmesh(body_obj, basis_coords)

    # Clean pure-quad selection from waist (z ~ 1.00m) down to ankles (z ~ 0.125m), excluding hands (|x| < 0.28)!
    def keep_face(f) -> bool:
        c = f.calc_center_median()
        return 0.125 <= c.z <= 0.995 and abs(c.x) < 0.28

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
    ankle_l_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) < 0.30 and sum(v.co.x for v in lp) / len(lp) < 0.0)
    ankle_r_loop = next(lp for lp in b_loops if sum(v.co.z for v in lp) / len(lp) < 0.30 and sum(v.co.x for v in lp) / len(lp) > 0.0)
    boundary_set = set(waist_loop) | set(ankle_l_loop) | set(ankle_r_loop)

    # Level waist & ankle rings cleanly BEFORE BVH relaxation (cover 100% of chartest painted shorts up to z=1.048m!)
    for v in waist_loop:
        v.co.z = 1.018 - 0.004 * smoothstep(-0.05, 0.06, v.co.y)
    for lp in (ankle_l_loop, ankle_r_loop):
        for v in lp:
            v.co.z = 0.128

    # Relax bare-skin leg anatomy (kneecap/calf sharpness & deep glute crease) into woven linen trouser tubes!
    for _ in range(4):
        new_pos = {}
        for v in bm.verts:
            if v in boundary_set:
                continue
            # Keep x=0 centerline symmetric while letting y/z relax smoothly across crotch & glute cleft!
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

    # Apply tailored linen trouser slack:
    # - Snugger at upper waist/hips (z > 0.90: +7.2..8.5mm so untucked shirts at +15..28mm layer cleanly over it!)
    # - Airy relaxed medieval linen drape through thighs, knees & calves (z in 0.20..0.82: +11..15mm + soft vertical folds!)
    for v in bm.verts:
        p = v.co.copy()
        loc, n_body, _, _ = body_bvh.find_nearest(p)
        out_n = v.normal.normalized()
        if n_body is not None and out_n.dot(n_body) > 0.15:
            out_n = (out_n * 0.55 + n_body * 0.45).normalized()

        leg_drape = bell(p.z, 0.50, 0.36)  # Peaks around mid-thigh / knee / upper calf
        glute_bridge = bell(p.x, 0.0, 0.045) * bell(p.z, 0.87, 0.09) * smoothstep(-0.06, -0.14, p.y) * 0.0085
        leg_cx = -0.10 if p.x < 0.0 else +0.10
        leg_ang = math.atan2(p.x - leg_cx, p.y + 0.04)
        folds = (0.5 + 0.5 * math.cos(leg_ang * 5.0 + p.z * 10.0)) * 0.0032 * leg_drape

        slack = 0.0092 + 0.0078 * leg_drape + glute_bridge + folds
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

    # 1) Stitched Gathered Linen Waistband (z = 0.992 -> 1.016m) + Solid Inward Rim
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

    # 2) Turned-Up / Rolled Linen Ankle Cuffs (z = 0.128 -> 0.112 -> 0.094m)
    for a_loop in (ankle_l_loop, ankle_r_loop):
        cx = sum(v.co.x for v in a_loop) / len(a_loop)
        cy = sum(v.co.y for v in a_loop) / len(a_loop)

        av0, av1 = a_loop[0], a_loop[1]
        ae0 = bm.edges.get((av0, av1))
        flip_a = False
        if ae0 and ae0.link_faces:
            lps = list(ae0.link_faces[0].loops)
            flip_a = any(lp.vert == av0 and lps[(i + 1) % len(lps)].vert == av1 for i, lp in enumerate(lps))

        def ankle_ring(z_val: float, slack_m: float, pleat_a: float = 0.0):
            orig = Vector((cx, cy, z_val))
            pts, r_dirs = [], []
            for idx_v, v in enumerate(a_loop):
                r_dir = Vector((v.co.x - cx, v.co.y - cy, 0.0)).normalized()
                r_dirs.append(r_dir)
                hit, _, _, _ = body_bvh.ray_cast(orig, r_dir, 0.14)
                r_skin = (hit - orig).length if hit is not None else 0.044
                pl = pleat_a * math.cos(idx_v * 2.0 * math.pi / len(a_loop) * 3.0)
                pts.append(orig + r_dir * (r_skin + slack_m + pl))
            return pts, r_dirs

        puff_pts, rad_dirs = ankle_ring(0.132, 0.0145, pleat_a=0.0025)
        for v, pt in zip(a_loop, puff_pts):
            v.co = pt

        prev_a = a_loop
        # Rolled cuff fold: z=0.114 (+12.5mm) -> z=0.095 (+11.5mm, clear above foot instep!)
        for z_st, slk, is_cuff in [(0.114, 0.0128, True), (0.095, 0.0118, True)]:
            c_pts, _ = ankle_ring(z_st, slk)
            new_a = [bm.verts.new(pt) for pt in c_pts]
            na = len(a_loop)
            for i in range(na):
                j = (i + 1) % na
                fq = bm.faces.new(
                    (prev_a[j], prev_a[i], new_a[i], new_a[j])
                    if flip_a
                    else (prev_a[i], prev_a[j], new_a[j], new_a[i])
                )
                if is_cuff:
                    border_faces.append(fq)
            prev_a = new_a

        arf, asf, arv, adv = extrude_closed_rim_inward(
            bm, prev_a, [-rd for rd in rad_dirs], rim_thick=0.0048, sink_depth=0.0070
        )
        border_faces.extend(arf)
        shadow_faces.extend(asf)
        interior_verts.update(arv)
        interior_verts.update(adv)

    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0070, min_face_gap=0.0056
    )

    # 3) Rustic Front Hemp Waist Drawstring Tie (knot + 2 short hanging ends at center-front waist!)
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

    # Small horizontal tie knot + Left & Right hanging cord tails
    add_cord_tube([knot_p + Vector((-0.011, -0.001, 0.0)), knot_p + Vector((0.0, 0.0015, 0.0)), knot_p + Vector((0.011, -0.001, 0.0))], 0.0030)
    add_cord_tube([knot_p, knot_p + Vector((-0.009, 0.002, -0.028)), knot_p + Vector((-0.014, 0.001, -0.058))], 0.0022)
    add_cord_tube([knot_p, knot_p + Vector((+0.008, 0.002, -0.025)), knot_p + Vector((+0.012, 0.001, -0.050))], 0.0022)

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # 4) Clean Cylindrical Trouser UVs (Pelvis unwrap + Left/Right Leg cylindrical unwrap!)
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
            cx = 0.0 if c.z > 0.50 else (-0.10 if c.x < 0.0 else +0.10)
            u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x - cx, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.90, lp.vert.co) for lp in f.loops]
            if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.5:
                u_raws = [(lp, u + 0.90 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.160 if p.z < c.z else 0.212)
            continue

        c = f.calc_center_median()
        if c.z < 0.78:
            # Individual leg tube unwrap around leg center (cx, -0.035) -> zero inner-thigh stretching!
            is_left = c.x < 0.0
            cx = -0.105 if is_left else +0.105
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                u_raw = (math.atan2(p.x - cx, p.y + 0.035) + math.pi) / (2 * math.pi)
                v_val = clamp(0.23 + (p.z - 0.095) / (0.78 - 0.095) * 0.52, 0.23, 0.76)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.55:
                u_list = [(lp, u + 1.0 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            u_base = 0.03 if is_left else 0.52
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_base + 0.44 * u_val, 0.02, 0.98), v_val)
        else:
            # Pelvis / hips cylindrical unwrap around (0, -0.045)
            u_list = []
            for lp in f.loops:
                p = lp.vert.co
                u_raw = 0.50 + (math.atan2(p.x, p.y + 0.045) / (2 * math.pi)) * 0.90
                v_val = clamp(0.75 + (p.z - 0.78) / (1.02 - 0.78) * 0.23, 0.74, 0.98)
                u_list.append((lp, u_raw, v_val))
            if max(x[1] for x in u_list) - min(x[1] for x in u_list) > 0.48:
                u_list = [(lp, u + 0.90 if u < 0.5 else u, v) for (lp, u, v) in u_list]
            for lp, u_val, v_val in u_list:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("CalcaLinho")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("CalcaLinho", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_interior


def render_pants_previews(pants_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render:
    1) preview_pants_angles.png: 4 lower-body / full-leg angles (Front, 3/4, Side, 3/4 Back)
    2) preview_pants_body_types.png: 5 body types (including shirt+pants combo on #1 & Female Max Curvy glutes on #3!) + isolated pants!
    """
    scene = bpy.context.scene
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"
    if hasattr(scene, "eevee"):
        if hasattr(scene.eevee, "samples"):
            scene.eevee.samples = 12
        elif hasattr(scene.eevee, "taa_render_samples"):
            scene.eevee.taa_render_samples = 12

    world = bpy.data.worlds.new("PantsWorld")
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.052, 0.060, 0.075, 1.0)

    lights = []
    for lname, loc, energy, col in [
        ("PtKey", (-2.2, 3.8, 2.8), 3.4, (1.0, 0.97, 0.92)),
        ("PtFill", (2.8, 2.8, 1.8), 1.8, (0.82, 0.88, 1.0)),
        ("PtRim", (0.2, -3.6, 2.4), 2.3, (0.94, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy, ldata.color = energy, col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 0.70)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("PtCam")
    cam = bpy.data.objects.new("PtCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    fem_mat = None
    fem_img = bpy.data.images.get("Chartest-Female-v01-128.png")
    if fem_img is not None:
        fem_mat = bpy.data.materials.new("PtFemSkin")
        fem_mat.use_nodes = True
        ns, ls = fem_mat.node_tree.nodes, fem_mat.node_tree.links
        ns.clear()
        mo, bs, ti = ns.new("ShaderNodeOutputMaterial"), ns.new("ShaderNodeBsdfPrincipled"), ns.new("ShaderNodeTexImage")
        bs.inputs["Roughness"].default_value = 0.9
        ti.image, ti.interpolation = fem_img, "Closest"
        ls.new(ti.outputs["Color"], bs.inputs["Base Color"])
        ls.new(bs.outputs["BSDF"], mo.inputs["Surface"])

    # Also import approved CamisaLinhoClara.glb temporarily to showcase Shirt + Pants layering on Character #1!
    shirt_glb = pathlib.Path(r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\clothes\CamisaLinhoClara.glb")
    combo_shirt = None
    imported_extras = []
    if shirt_glb.exists():
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(shirt_glb))
        after = [o for o in bpy.data.objects if o not in before]
        imported_extras = after
        for o in after:
            o.hide_render = True
            if o.type == "MESH" and "CamisaLinhoClara" in o.name:
                combo_shirt = o

    deps = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = arm_obj.hide_render = pants_obj.hide_render = True

    def spawn(x_p, yaw, include_body=True, with_shirt=False, is_female=False, breasts=0.0, glutes=0.0, height_m=1.82, sk=None):
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
        arm_obj.update_tag()
        body_obj.update_tag()
        pants_obj.update_tag()
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

        ev_p = pants_obj.evaluated_get(deps)
        mp = bpy.data.meshes.new_from_object(ev_p, preserve_all_data_layers=True, depsgraph=deps)
        op = bpy.data.objects.new("SnapPants", mp)
        scene.collection.objects.link(op)
        op.location, op.rotation_euler = (x_p, 0.0, 0.0), rot
        objs.append(op)

        if with_shirt and combo_shirt is not None:
            # Evaluate combo_shirt in relaxed pose on Base Male to prove 0 shirt-vs-pants clipping!
            combo_shirt.modifiers["Armature"].object = arm_obj
            deps.update()
            ev_sh = combo_shirt.evaluated_get(deps)
            msh = bpy.data.meshes.new_from_object(ev_sh, preserve_all_data_layers=True, depsgraph=deps)
            osh = bpy.data.objects.new("SnapComboShirt", msh)
            scene.collection.objects.link(osh)
            osh.location, osh.rotation_euler = (x_p, 0.0, 0.0), rot
            objs.append(osh)

        return objs

    def clean(objs):
        for o in objs:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            if m and m.users == 0:
                bpy.data.meshes.remove(m)

    # Sheet 1: 4 Angles focused on Waist-to-Feet (Front 0°, 3/4 +34°, Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(1.55, 0.0), (0.50, 34.0), (-0.50, -90.0), (-1.48, -142.0)]:
        snaps.extend(spawn(x_p, yaw, include_body=True, is_female=False))
    scene.render.resolution_x, scene.render.resolution_y = 1720, 780
    cam.location = (0.04, 4.0, 0.64)
    cam.rotation_euler = (Vector((0.04, 0.0, 0.62)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.15
    scene.render.filepath = str(out_dir / "preview_pants_angles.png")
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    # Sheet 2: Full-Body Auto-Fit across 5 Body Types (with #1 wearing CamisaLinhoClara + CalcaLinho combo!) + Isolated Pants!
    snaps = []
    snaps.extend(spawn(2.88, 24.0, with_shirt=True, is_female=False, height_m=1.82))
    snaps.extend(spawn(1.78, 28.0, is_female=True, breasts=1.05, glutes=0.90, height_m=1.74))
    snaps.extend(spawn(0.74, -118.0, is_female=True, breasts=1.95, glutes=1.95, height_m=1.74))  # Rear-3/4 to show Max Glutes fit!
    snaps.extend(
        spawn(-0.36, 24.0, is_female=False, height_m=1.95, sk={"Body_Muscle": 1.0, "Thighs_Mass": 0.75, "Calves_Mass": 0.75})
    )
    snaps.extend(
        spawn(-1.56, 28.0, is_female=False, height_m=1.75, sk={"Body_Heavy_Fat": 1.0, "Abdomen_Belly": 0.70, "Hips_Width": 0.55})
    )
    iso = spawn(-2.72, 26.0, include_body=False)
    for o in iso:
        o.scale = (1.32, 1.32, 1.32)
        o.location = (-2.72, 0.0, 0.16)
    snaps.extend(iso)

    scene.render.resolution_x, scene.render.resolution_y = 1920, 800
    cam.location = (0.04, 5.5, 0.96)
    cam.rotation_euler = (Vector((0.04, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 7.15
    scene.render.filepath = str(out_dir / "preview_pants_body_types.png")
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    for o in imported_extras:
        if o.name in bpy.data.objects:
            bpy.data.objects.remove(o, do_unlink=True)

    set_relaxed_preview_pose(arm_obj, relax_deg=0.0)
    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"], arm_obj["is_female"], arm_obj["breasts_size"], arm_obj["glutes_size"] = 1.82, False, 0.0, 0.0
    body_obj.hide_render = arm_obj.hide_render = pants_obj.hide_render = False


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, body_bvh, basis_coords = open_clean_chartest()
    tex_img = generate_linen_pants_texture(TECIDO_GROSSO_PNG, OUT_DIR / "calca_linho_128.png")
    pants_obj, interior_idx = build_linen_pants_mesh(body_obj, arm_obj, body_bvh, basis_coords)
    pants_obj.parent = arm_obj
    export_uv_overlay(pants_obj, OUT_DIR / "calca_linho_uv.png")
    autofit_garment_to_chartest(pants_obj, body_obj, arm_obj, interior_idx)
    verify_garment_and_export_glb(pants_obj, body_obj, arm_obj, tex_img, OUT_DIR / "CalcaLinho.glb", interior_idx)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_DIR / "calca_linho.blend"))
    render_pants_previews(pants_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CalcaLinho built, audited & rendered!")


if __name__ == "__main__":
    main()
