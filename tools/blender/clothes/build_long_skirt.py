"""Build Grimora low-poly PSX Ankle-Length Pleated Long Skirt (SaiaLonga) using grimora_cloth_kit."""

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
    enforce_mesh_clearance_over_bvh,
    export_uv_overlay,
    extrude_closed_rim_inward,
    open_clean_chartest,
    set_relaxed_preview_pose,
    smoothstep,
    verify_garment_and_export_glb,
)

OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\clothing_long_skirt"
)


def generate_long_skirt_texture(src_png: pathlib.Path, out_png: pathlib.Path) -> bpy.types.Image:
    """Derive 128x128 4-zone PSX dark-fantasy midnight-indigo / slate-navy woven skirt atlas:
    - V in [0.30 .. 1.00]: Deep midnight-indigo / slate-navy woven wool-linen skirt fabric
    - V in [0.19 .. 0.29]: Antique cream/silver woven floral-geometric bottom hem border
    - V in [0.08 .. 0.18]: Dark leather/woven gathered waistband & side ribbon tie
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

    # Midnight-indigo / muted slate-navy wool-linen
    s_dark = (0.12, 0.15, 0.23)
    s_mid = (0.20, 0.25, 0.37)
    s_hi = (0.30, 0.36, 0.50)

    # Antique cream-parchment woven hem trim
    t_dark = (0.58, 0.54, 0.46)
    t_hi = (0.86, 0.82, 0.73)

    # Dark brown waistband & tie
    w_dark = (0.14, 0.09, 0.07)
    w_hi = (0.31, 0.20, 0.14)

    inner_col = (0.07, 0.08, 0.12)

    for y in range(h):
        v = (y + 0.5) / h
        for x in range(w):
            idx = (y * w + x) * 4
            tn = clamp((sig_grid[y][x] - v_lo) / v_span, 0.0, 1.0)

            if v < 0.075:
                f = 0.84 + 0.32 * tn
                out_px[idx : idx + 4] = [inner_col[0] * f, inner_col[1] * f, inner_col[2] * f, 1.0]
            elif v < 0.185:
                rib = 0.90 + 0.16 * math.cos(x * math.pi * 0.5)
                tw = clamp(tn * rib, 0.0, 1.0)
                r = w_dark[0] + (w_hi[0] - w_dark[0]) * tw
                g = w_dark[1] + (w_hi[1] - w_dark[1]) * tw
                b = w_dark[2] + (w_hi[2] - w_dark[2]) * tw
                if y in (11, 22) and ((x // 2) % 2 == 0):
                    r, g, b = r * 1.28, g * 1.24, b * 1.18
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            elif v < 0.295:
                # Cream woven diamond/scallop border on deep indigo backing
                dy_b = y - 30
                diamond = (abs((x % 6) - 3) + abs(dy_b)) <= 3 or y in (25, 36)
                if diamond:
                    tc = clamp(tn * 0.75 + 0.25, 0.0, 1.0)
                    r = t_dark[0] + (t_hi[0] - t_dark[0]) * tc
                    g = t_dark[1] + (t_hi[1] - t_dark[1]) * tc
                    b = t_dark[2] + (t_hi[2] - t_dark[2]) * tc
                else:
                    r, g, b = s_dark[0] * (0.9 + 0.3 * tn), s_dark[1] * (0.9 + 0.3 * tn), s_dark[2] * (0.9 + 0.3 * tn)
                out_px[idx : idx + 4] = [clamp(r), clamp(g), clamp(b), 1.0]
            else:
                if tn < 0.5:
                    k = tn / 0.5
                    r = s_dark[0] + (s_mid[0] - s_dark[0]) * k
                    g = s_dark[1] + (s_mid[1] - s_dark[1]) * k
                    b = s_dark[2] + (s_mid[2] - s_dark[2]) * k
                else:
                    k = (tn - 0.5) / 0.5
                    r = s_mid[0] + (s_hi[0] - s_mid[0]) * k
                    g = s_mid[1] + (s_hi[1] - s_mid[1]) * k
                    b = s_mid[2] + (s_hi[2] - s_mid[2]) * k
                hem_shade = 1.0 - 0.10 * smoothstep(0.42, 0.30, v)
                out_px[idx : idx + 4] = [clamp(r * hem_shade), clamp(g * hem_shade), clamp(b * hem_shade), 1.0]

    img = bpy.data.images.new("saia_longa_128", width=w, height=h, alpha=True)
    img.pixels[:] = out_px
    img.filepath_raw = str(out_png)
    img.file_format = "PNG"
    img.save()
    return img


def build_long_skirt_mesh(body_obj, arm_obj, body_bvh, basis_coords):
    """Construct a continuous 32-column pleated ankle-length A-line skirt (z = 1.050m down to z = 0.125m)."""
    bm = bmesh.new()
    n_cols = 32
    n_pleats = 10  # 10 flowing vertical pleats around circumference

    def ring_point(ang: float, z_lvl: float, extra_flare: float, pleat_amp: float) -> Vector:
        # Measure actual torso/leg cross-section near z_lvl (strictly excluding A-pose hands |x| > 0.26!)
        band = [p for p in basis_coords if abs(p.z - z_lvl) < 0.052 and abs(p.x) < 0.26]
        xmax_b = max((abs(p.x) for p in band), default=0.190)
        ymax_b = max((p.y for p in band), default=0.060)
        ymin_b = min((p.y for p in band), default=-0.150)

        y_center = 0.5 * (ymax_b + ymin_b)
        rx = xmax_b + extra_flare
        ry_front = (ymax_b - y_center) + extra_flare + 0.005
        ry_back = (y_center - ymin_b) + extra_flare + 0.007

        sx, cy = math.sin(ang), math.cos(ang)
        pleat = math.cos(ang * n_pleats) * pleat_amp
        px = sx * (rx + pleat)
        py = y_center + cy * ((ry_front if cy >= 0.0 else ry_back) + pleat)

        # Raycast check on closed pelvis/waist (z > 0.80, max_dist=0.25) so hip/waist contours fit snugly!
        if z_lvl > 0.80:
            horiz = Vector((px, py - y_center, 0.0))
            if horiz.length > 1e-5:
                hd = horiz.normalized()
                orig = Vector((0.0, y_center, z_lvl))
                hit, _, _, _ = body_bvh.ray_cast(orig, hd, 0.25)
                if hit is not None:
                    # Above z=0.98 (waistband), follow exact waist cross-section + extra_flare!
                    r_hit = (hit - orig).length + extra_flare + pleat
                    if z_lvl >= 0.98 or r_hit > horiz.length:
                        pt_r = orig + hd * r_hit
                        px, py = pt_r.x, pt_r.y

        # Gentle feminine hem wave at bottom
        z_wave = pleat * 0.22 * smoothstep(0.55, 0.12, z_lvl)
        return Vector((px, py, z_lvl + z_wave))

    # Tiers from top of waistband (z=1.050m, covers 100% of base shorts & tucks under BlusaFeminina peplum!)
    # down to ankle hem (z=0.125m) with progressive A-line bell flare & deepening vertical pleats!
    tiers = [
        # (z_lvl, extra_flare, pleat_amp, zone)
        (1.050, 0.0078, 0.0000, "waist"),
        (1.002, 0.0082, 0.0006, "waist"),
        (0.935, 0.0105, 0.0018, "body"),
        (0.845, 0.0175, 0.0034, "body"),
        (0.735, 0.0285, 0.0048, "body"),
        (0.615, 0.0420, 0.0062, "body"),
        (0.490, 0.0540, 0.0074, "body"),
        (0.365, 0.0660, 0.0085, "body"),
        (0.245, 0.0780, 0.0095, "body"),
        (0.162, 0.0870, 0.0102, "body"),
        (0.125, 0.0915, 0.0106, "trim"),
    ]

    rings = []
    # Measure hip baseline at z=0.90m so the skirt flows in a pure unbroken A-line cone down to the ankles!
    hip_band = [p for p in basis_coords if 0.82 <= p.z <= 0.96 and abs(p.x) < 0.26]
    hip_rx0 = max((abs(p.x) for p in hip_band), default=0.196) + 0.012
    hip_yf0 = max((p.y for p in hip_band), default=0.062) + 0.014
    hip_yb0 = min((p.y for p in hip_band), default=-0.148) - 0.015

    for r_idx, (z_lvl, flr, pl_a, _) in enumerate(tiers):
        band = [p for p in basis_coords if abs(p.z - z_lvl) < 0.052 and abs(p.x) < 0.26]
        body_rx = max((abs(p.x) for p in band), default=0.185) + 0.016
        body_yf = max((p.y for p in band), default=0.055) + 0.016
        body_yb = min((p.y for p in band), default=-0.140) - 0.016

        t_cone = clamp((0.935 - z_lvl) / (0.935 - 0.125), 0.0, 1.0)
        t_bell = 0.78 * t_cone + 0.22 * (t_cone ** 1.35)
        xmax_b = max(body_rx, hip_rx0 + 0.084 * t_bell)
        ymax_b = max(body_yf, hip_yf0 + 0.070 * t_bell)
        ymin_b = min(body_yb, hip_yb0 - 0.056 * t_bell)

        y_c = 0.5 * (ymax_b + ymin_b)
        ry_f = ymax_b - y_c
        ry_bk = y_c - ymin_b

        ring = []
        for k in range(n_cols):
            ang = k * 2.0 * math.pi / n_cols
            if r_idx <= 2:
                ring.append(bm.verts.new(ring_point(ang, z_lvl, flr, pl_a)))
            else:
                pleat = math.cos(ang * n_pleats) * pl_a
                sx, cy = math.sin(ang), math.cos(ang)
                px = sx * (xmax_b + pleat)
                py = y_c + cy * ((ry_f if cy >= 0.0 else ry_bk) + pleat)
                z_wave = pleat * 0.22 * smoothstep(0.55, 0.12, z_lvl)
                ring.append(bm.verts.new(Vector((px, py, z_lvl + z_wave))))
        rings.append(ring)

    waist_faces = []
    trim_faces = []
    shadow_faces = []
    interior_verts = set()

    for r_i in range(len(tiers) - 1):
        ra, rb = rings[r_i], rings[r_i + 1]
        zone = tiers[r_i + 1][3]
        for k in range(n_cols):
            kn = (k + 1) % n_cols
            # Outward normal: ang increases clockwise from +Y (+X is sin(ang)), so (ra[kn], ra[k], rb[k], rb[kn]) faces outward!
            fq = bm.faces.new((ra[kn], ra[k], rb[k], rb[kn]))
            if r_i == 0:
                waist_faces.append(fq)
            elif zone == "trim":
                trim_faces.append(fq)

    # Solid inward waistband rim at top (rings[0])
    wb_in_dirs = [Vector((-v.co.x, -(v.co.y + 0.045), -0.15)).normalized() for v in rings[0]]
    wrf, wsf, wrv, wdv = extrude_closed_rim_inward(bm, rings[0], wb_in_dirs, rim_thick=0.0046, sink_depth=0.0065)
    waist_faces.extend(wrf)
    shadow_faces.extend(wsf)
    interior_verts.update(wrv)
    interior_verts.update(wdv)

    # Solid inward folded cloth rim + dark interior shadow lip at ankle hem (rings[-1])!
    hem_in_dirs = [Vector((-v.co.x, -(v.co.y + 0.045), 0.18)).normalized() for v in rings[-1]]
    hrf, hsf, hrv, hdv = extrude_closed_rim_inward(bm, rings[-1], hem_in_dirs, rim_thick=0.0058, sink_depth=0.0095)
    trim_faces.extend(hrf)
    shadow_faces.extend(hsf)
    interior_verts.update(hrv)
    interior_verts.update(hdv)

    enforce_mesh_clearance_over_bvh(
        bm, body_bvh, skip_verts=interior_verts, min_vert_gap=0.0072, min_face_gap=0.0058
    )

    # Subtle rustic side-waist ribbon tie on left hip (x < 0, ang ~ -0.70 rad) for extra silhouette charm!
    tie_v = rings[1][int(round(n_cols * 0.88))].co + Vector((-0.004, 0.004, 0.012))

    def add_ribbon(pts: list[Vector], rad: float = 0.0028):
        rgs = []
        for i_p, pt in enumerate(pts):
            ax = (pts[min(i_p + 1, len(pts) - 1)] - pts[max(i_p - 1, 0)]).normalized()
            up = Vector((0.0, 1.0, 0.0)) if abs(ax.y) < 0.9 else Vector((0.0, 0.0, 1.0))
            s1 = ax.cross(up).normalized() * rad
            s2 = ax.cross(s1).normalized() * (rad * 0.55)
            rv = [bm.verts.new(pt + s1), bm.verts.new(pt + s2), bm.verts.new(pt - s1), bm.verts.new(pt - s2)]
            rgs.append(rv)
            interior_verts.update(rv)
        for ri in range(len(rgs) - 1):
            a_r, b_r = rgs[ri], rgs[ri + 1]
            for k in range(4):
                kn = (k + 1) % 4
                waist_faces.append(bm.faces.new((a_r[k], a_r[kn], b_r[kn], b_r[k])))

    add_ribbon([tie_v, tie_v + Vector((-0.012, 0.006, -0.048)), tie_v + Vector((-0.018, 0.008, -0.105))], 0.0034)
    add_ribbon([tie_v, tie_v + Vector((-0.005, 0.012, -0.042)), tie_v + Vector((-0.008, 0.015, -0.088))], 0.0030)

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.verts.ensure_lookup_table()
    bm.faces.ensure_lookup_table()

    # Clean Cylindrical UVs (~140 px/m texel density via 2 vertical skirt bands + trim + waistband!)
    uv_lay = bm.loops.layers.uv.new("UVMap")
    shd_set, wst_set, trm_set = set(shadow_faces), set(waist_faces), set(trim_faces)

    for f in bm.faces:
        if f in shd_set:
            for lp in f.loops:
                lp[uv_lay].uv = (0.50, 0.035)
            continue
        u_raws = [(lp, 0.50 + math.atan2(lp.vert.co.x, lp.vert.co.y + 0.045) / (2 * math.pi) * 0.92, lp.vert.co) for lp in f.loops]
        if max(x[1] for x in u_raws) - min(x[1] for x in u_raws) > 0.48:
            u_raws = [(lp, u + 0.92 if u < 0.5 else u, p) for (lp, u, p) in u_raws]
        c = f.calc_center_median()
        if f in wst_set:
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.095 if p.z < c.z else 0.172)
        elif f in trm_set:
            for lp, u_val, p in u_raws:
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), 0.200 if p.z < c.z else 0.285)
        else:
            for lp, u_val, p in u_raws:
                if c.z >= 0.56:
                    v_val = clamp(0.32 + (p.z - 0.55) / (1.01 - 0.55) * 0.65, 0.31, 0.98)
                else:
                    v_val = clamp(0.32 + (p.z - 0.15) / (0.56 - 0.15) * 0.65, 0.31, 0.98)
                lp[uv_lay].uv = (clamp(u_val, 0.02, 0.98), v_val)

    final_interior = {v.index for v in interior_verts if v.is_valid}
    mesh = bpy.data.meshes.new("SaiaLonga")
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = False

    obj = bpy.data.objects.new("SaiaLonga", mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj, final_interior


def smooth_skirt_bustle_drape(skirt_obj, body_obj, basis_coords, interior_indices: set[int]):
    """Make Glutes_Mass, Feminine_Body & Hips_Width flow as a continuous A-line curtain down to the ankles (no knee pinch!)."""
    basis_s = skirt_obj.data.shape_keys.key_blocks[0].data

    # 1) Lateral hip curtain for hip/thigh-widening morphs (carries hip widening smoothly down to ankle!)
    for kname in ("Feminine_Body", "Hips_Width", "Body_Heavy_Fat", "Body_Muscle", "Thighs_Mass", "Legs_Full"):
        skb = skirt_obj.data.shape_keys.key_blocks.get(kname)
        b_kb = body_obj.data.shape_keys.key_blocks.get(kname)
        if skb is None or b_kb is None:
            continue
        peak_dx = max((abs(b_kb.data[i].co.x) - abs(bp.x) for i, bp in enumerate(basis_coords) if 0.78 <= bp.z <= 0.96), default=0.0)
        if peak_dx > 0.0:
            for vi in range(len(skirt_obj.data.vertices)):
                if vi in interior_indices:
                    continue
                p = basis_s[vi].co
                if p.z < 0.88 and abs(p.x) > 0.02:
                    cur_d = skb.data[vi].co - p
                    sx = 1.0 if p.x > 0 else -1.0
                    lat_w = abs(p.x) / max(math.hypot(p.x, p.y + 0.045), 1e-4)
                    want_dx = peak_dx * (0.85 + 0.20 * smoothstep(0.88, 0.12, p.z)) * lat_w
                    skb.data[vi].co = Vector((p.x + sx * max(sx * cur_d.x, want_dx), p.y + cur_d.y, p.z + cur_d.z * 0.4))

    # 2) Back bustle curtain for Glutes_Mass
    skb = skirt_obj.data.shape_keys.key_blocks.get("Glutes_Mass")
    b_kb = body_obj.data.shape_keys.key_blocks.get("Glutes_Mass")
    if skb is None or b_kb is None:
        return
    peak_neg_dy = min((b_kb.data[i].co.y - bp.y for i, bp in enumerate(basis_coords) if 0.78 <= bp.z <= 0.98 and bp.y < -0.05), default=0.0)
    for vi in range(len(skirt_obj.data.vertices)):
        if vi in interior_indices:
            continue
        p = basis_s[vi].co
        if p.z < 0.98 and p.y < -0.03:
            cur_d = skb.data[vi].co - p
            t_h = smoothstep(0.12, 0.88, p.z)
            mask = smoothstep(-0.02, -0.10, p.y) * smoothstep(0.26, 0.10, abs(p.x))
            curtain_dy = peak_neg_dy * (0.45 + 0.55 * (t_h ** 0.75)) * mask
            skb.data[vi].co = Vector((p.x + cur_d.x, p.y + min(cur_d.y, curtain_dy), p.z + cur_d.z * 0.40))


def render_long_skirt_previews(skirt_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render 4 angles on Female Body + 5 Body Types (with #1 wearing BlusaFeminina + SaiaLonga full outfit!) + Isolated Skirt."""
    scene = bpy.context.scene
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"
    if hasattr(scene, "eevee"):
        if hasattr(scene.eevee, "samples"):
            scene.eevee.samples = 12
        elif hasattr(scene.eevee, "taa_render_samples"):
            scene.eevee.taa_render_samples = 12

    world = bpy.data.worlds.new("SkirtWorld")
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.052, 0.060, 0.075, 1.0)

    lights = []
    for lname, loc, energy, col in [
        ("SkKey", (-2.2, 3.8, 2.8), 3.4, (1.0, 0.97, 0.92)),
        ("SkFill", (2.8, 2.8, 1.8), 1.8, (0.82, 0.88, 1.0)),
        ("SkRim", (0.2, -3.6, 2.4), 2.3, (0.94, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy, ldata.color = energy, col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 0.70)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("SkCam")
    cam = bpy.data.objects.new("SkCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    fem_mat = None
    fem_img = bpy.data.images.get("Chartest-Female-v01-128.png")
    if fem_img is not None:
        fem_mat = bpy.data.materials.new("SkFemSkin")
        fem_mat.use_nodes = True
        ns, ls = fem_mat.node_tree.nodes, fem_mat.node_tree.links
        ns.clear()
        mo, bs, ti = ns.new("ShaderNodeOutputMaterial"), ns.new("ShaderNodeBsdfPrincipled"), ns.new("ShaderNodeTexImage")
        bs.inputs["Roughness"].default_value = 0.9
        ti.image, ti.interpolation = fem_img, "Closest"
        ls.new(ti.outputs["Color"], bs.inputs["Base Color"])
        ls.new(bs.outputs["BSDF"], mo.inputs["Surface"])

    # Import approved BlusaFeminina.glb temporarily to showcase complete Female Outfit (#1)!
    blouse_glb = pathlib.Path(r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\clothes\BlusaFeminina.glb")
    combo_blouse = None
    imported_extras = []
    if blouse_glb.exists():
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=str(blouse_glb))
        after = [o for o in bpy.data.objects if o not in before]
        imported_extras = after
        for o in after:
            o.hide_render = True
            if o.type == "MESH" and "BlusaFeminina" in o.name:
                combo_blouse = o

    deps = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = arm_obj.hide_render = skirt_obj.hide_render = True

    def spawn(x_p, yaw, include_body=True, with_blouse=False, is_female=True, breasts=1.10, glutes=0.95, height_m=1.74, sk=None):
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
        skirt_obj.update_tag()
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

        ev_s = skirt_obj.evaluated_get(deps)
        ms = bpy.data.meshes.new_from_object(ev_s, preserve_all_data_layers=True, depsgraph=deps)
        os_o = bpy.data.objects.new("SnapSkirt", ms)
        scene.collection.objects.link(os_o)
        os_o.location, os_o.rotation_euler = (x_p, 0.0, 0.0), rot
        objs.append(os_o)

        if with_blouse and combo_blouse is not None:
            combo_blouse.modifiers["Armature"].object = arm_obj
            if combo_blouse.data.shape_keys:
                for kb in body_obj.data.shape_keys.key_blocks[1:]:
                    if kb.name in combo_blouse.data.shape_keys.key_blocks:
                        combo_blouse.data.shape_keys.key_blocks[kb.name].value = kb.value
                # Also sync driven keys from evaluated body!
                ev_b_sk = body_obj.evaluated_get(deps)
                combo_blouse.data.shape_keys.key_blocks["Feminine_Body"].value = 1.0 if is_female else 0.0
                combo_blouse.data.shape_keys.key_blocks["Female_Breasts"].value = breasts if is_female else 0.0
                combo_blouse.data.shape_keys.key_blocks["Glutes_Mass"].value = glutes
            combo_blouse.update_tag()
            deps.update()
            ev_bl = combo_blouse.evaluated_get(deps)
            mbl = bpy.data.meshes.new_from_object(ev_bl, preserve_all_data_layers=True, depsgraph=deps)
            obl = bpy.data.objects.new("SnapBlouse", mbl)
            scene.collection.objects.link(obl)
            obl.location, obl.rotation_euler = (x_p, 0.0, 0.0), rot
            objs.append(obl)

        return objs

    def clean(objs):
        for o in objs:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            if m and m.users == 0:
                bpy.data.meshes.remove(m)

    # Sheet 1: 4 Angles on Female Body (Front 0°, 3/4 +34°, Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(1.55, 0.0), (0.50, 34.0), (-0.50, -90.0), (-1.48, -142.0)]:
        snaps.extend(spawn(x_p, yaw, include_body=True, is_female=True, breasts=1.10, glutes=0.95, height_m=1.74))
    scene.render.resolution_x, scene.render.resolution_y = 1720, 780
    cam.location = (0.04, 4.0, 0.64)
    cam.rotation_euler = (Vector((0.04, 0.0, 0.62)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.15
    scene.render.filepath = str(out_dir / "preview_skirt_angles.png")
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    # Sheet 2: Full Outfit (BlusaFeminina + SaiaLonga) + 5 Body Types + Isolated Skirt!
    snaps = []
    snaps.extend(spawn(2.88, 26.0, with_blouse=True, is_female=True, breasts=1.15, glutes=0.95, height_m=1.74))
    snaps.extend(spawn(1.78, -112.0, is_female=True, breasts=1.95, glutes=1.95, height_m=1.74))
    snaps.extend(spawn(0.74, 26.0, is_female=True, breasts=0.45, glutes=0.35, height_m=1.60, sk={"Body_Gaunt_Lean": 0.65}))
    snaps.extend(spawn(-0.36, 26.0, is_female=True, breasts=1.50, glutes=1.40, height_m=1.73, sk={"Body_Heavy_Fat": 0.85, "Hips_Width": 0.45}))
    snaps.extend(spawn(-1.54, 24.0, is_female=False, breasts=0.0, glutes=0.0, height_m=1.84))
    iso = spawn(-2.72, 26.0, include_body=False, is_female=False, breasts=0.0, glutes=0.0, height_m=1.82)
    for o in iso:
        o.scale = (1.26, 1.26, 1.26)
        o.location = (-2.72, 0.0, 0.16)
    snaps.extend(iso)

    scene.render.resolution_x, scene.render.resolution_y = 1920, 800
    cam.location = (0.04, 5.5, 0.94)
    cam.rotation_euler = (Vector((0.04, 0.0, 0.92)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 7.15
    scene.render.filepath = str(out_dir / "preview_skirt_body_types.png")
    bpy.ops.render.render(write_still=True)
    clean(snaps)

    for o in imported_extras:
        if o.name in bpy.data.objects:
            bpy.data.objects.remove(o, do_unlink=True)
    set_relaxed_preview_pose(arm_obj, relax_deg=0.0)
    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    arm_obj["height_m"], arm_obj["is_female"], arm_obj["breasts_size"], arm_obj["glutes_size"] = 1.82, False, 0.0, 0.0
    body_obj.hide_render = arm_obj.hide_render = skirt_obj.hide_render = False


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    body_obj, arm_obj, body_bvh, basis_coords = open_clean_chartest()
    tex_img = generate_long_skirt_texture(TECIDO_GROSSO_PNG, OUT_DIR / "saia_longa_128.png")
    skirt_obj, interior_idx = build_long_skirt_mesh(body_obj, arm_obj, body_bvh, basis_coords)
    skirt_obj.parent = arm_obj
    export_uv_overlay(skirt_obj, OUT_DIR / "saia_longa_uv.png")
    autofit_garment_to_chartest(skirt_obj, body_obj, arm_obj, interior_idx)
    smooth_skirt_bustle_drape(skirt_obj, body_obj, basis_coords, interior_idx)
    verify_garment_and_export_glb(skirt_obj, body_obj, arm_obj, tex_img, OUT_DIR / "SaiaLonga.glb", interior_idx)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT_DIR / "saia_longa.blend"))
    render_long_skirt_previews(skirt_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] SaiaLonga built, audited & rendered in 1 pass!")


if __name__ == "__main__":
    main()
