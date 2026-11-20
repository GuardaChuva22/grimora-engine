"""Build Grimora low-poly PSX Male Samurai Bun (CoqueSamuraiMasculino) fitted to chartest.

Generates in the conversation staging directory (prior to user approval):
- CoqueSamuraiMasculino.glb                 (embedded 128x128 PSX hair+cord texture, head_socket hierarchy)
- coque_samurai_masculino_128.png           (128x128 dark-fantasy PSX pulled-strand & cord-tie texture)
- coque_samurai_masculino_uv.png            (128x128 transparent UV overlay)
- coque_samurai_masculino.blend             (authoring scene parented to chartest_rig -> head)
- preview_samurai_bun_angles.png            (4-angle head close-up: Front, 3/4 Front, Pure Side, 3/4 Back)
- preview_samurai_bun_full_and_iso.png      (Full-body front/back on chartest + isolated 3D piece)
"""

import json
import math
import pathlib
import struct

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree


CHARTEST_BLEND = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\chartest.blend"
)
OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\hair_samurai_bun"
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


def slerp_dir(u0: Vector, u1: Vector, t: float) -> Vector:
    a = u0.normalized()
    b = u1.normalized()
    dot = clamp(a.dot(b), -0.999999, 0.999999)
    omega = math.acos(dot)
    if omega < 1e-5:
        return a.lerp(b, t).normalized()
    so = math.sin(omega)
    return ((math.sin((1.0 - t) * omega) / so) * a + (math.sin(t * omega) / so) * b).normalized()


def generate_psx_samurai_hair_texture(png_path: pathlib.Path, w: int = 128, h: int = 128):
    """Author a gritty 128x128 PSX dark-fantasy pulled-hair + leather/cord tie texture.

    UV layout convention:
    - V in [0.18 .. 0.98]: tautly pulled dark-brown hair strands converging from hairline (V=0.18)
      into the knot tie (V=0.98), also shared by the folded bun loop & short tail tuft.
    - V in [0.08 .. 0.16]: dark charcoal-leather / wrapped hemp cord band for the bun tie!
    - V in [0.00 .. 0.07]: deep dark interior scalp lining shadow.
    """
    px = [0.0] * (w * h * 4)

    def h2(ix, iy):
        n = (ix * 374761393 + iy * 668265263) & 0xFFFFFFFF
        n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
        return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0

    def periodic_noise_1d(u_val: float, period: int, seed_y: int) -> float:
        pos = (u_val % 1.0) * period
        i0 = int(math.floor(pos)) % period
        i1 = (i0 + 1) % period
        f = pos - math.floor(pos)
        t = f * f * (3.0 - 2.0 * f)
        return h2(i0, seed_y) * (1.0 - t) + h2(i1, seed_y) * t

    # Rich dark-fantasy espresso/umber warrior hair palette matching Grimora's pixel contrast
    root_col = (0.058, 0.040, 0.029)
    mid_col = (0.208, 0.138, 0.092)
    hi_col = (0.352, 0.246, 0.166)
    shadow_col = (0.040, 0.028, 0.020)

    for y in range(h):
        v = (y + 0.5) / h

        # 1. Interior dark scalp shadow strip (V < 0.075)
        if v < 0.075:
            for x in range(w):
                jit = 0.92 + 0.16 * h2(x, y)
                idx = (y * w + x) * 4
                px[idx : idx + 4] = [
                    shadow_col[0] * jit,
                    shadow_col[1] * jit,
                    shadow_col[2] * jit,
                    1.0,
                ]
            continue

        # 2. Wrapped dark leather/cord tie strip (0.075 <= V < 0.170)
        if v < 0.170:
            vt = (v - 0.075) / (0.170 - 0.075)
            # 3 horizontal wrap cords with crisp dark grooves between wraps
            cord_wave = 0.5 + 0.5 * math.sin(vt * math.pi * 3.0)
            edge_ao = smoothstep(0.0, 0.18, vt) * smoothstep(1.0, 0.82, vt)
            for x in range(w):
                twist = 0.88 + 0.24 * h2((x + y // 2) // 2, y // 2)
                checker = 1.05 if ((x + y) & 1) == 0 else 0.95
                lum = (0.45 + 0.65 * cord_wave) * edge_ao * twist * checker
                # Dark warm charcoal-leather / blackened cord tone contrasting cleanly with brown hair
                r = clamp(0.095 * lum + 0.022, 0.025, 0.18)
                g = clamp(0.078 * lum + 0.018, 0.020, 0.15)
                b = clamp(0.066 * lum + 0.016, 0.018, 0.13)
                idx = (y * w + x) * 4
                px[idx : idx + 4] = [r, g, b, 1.0]
            continue

        # 3. Tautly pulled hair strands (V in [0.170 .. 1.00])
        vh = (v - 0.170) / (1.0 - 0.170)  # normalized 0 (hairline) -> 1 (knot root)
        vq = (int(y // 2) + 0.5) / (h / 2.0)

        for x in range(w):
            u_raw = (x + 0.5) / w
            # Tauter, straighter tensioned fibers (very slight wave so strands look pulled tight!)
            wave = (periodic_noise_1d(u_raw, 18, int(y // 14) + 17) - 0.5) * 0.008
            u = (u_raw + wave) % 1.0

            broad_lock = periodic_noise_1d(u, 16, 5)
            med_lock = periodic_noise_1d(u, 36, 13)
            fine_strand = periodic_noise_1d(u, 64, 29 + (y // 18))
            micro_stroke = h2(x, y // 3)

            strand = (
                0.32 * broad_lock
                + 0.34 * med_lock
                + 0.22 * fine_strand
                + 0.12 * micro_stroke
            )

            # Anisotropic sheen band across crown/bun crest (vh ~ 0.52..0.72)
            hi_center = 0.58 + 0.12 * (periodic_noise_1d(u, 20, 53) - 0.5)
            knot_ao = smoothstep(0.84, 0.99, vh)     # darkens tightly where hair pinches into cord wrap
            hairline_ao = smoothstep(0.18, 0.01, vh)  # natural root darkening at forehead/sideburns/nape
            spec_band = bell(vh, hi_center, 0.24) * smoothstep(0.42, 0.78, strand)

            shade = 0.62 + 0.72 * strand
            r = mid_col[0] * shade + (hi_col[0] - mid_col[0]) * spec_band * 0.88
            g = mid_col[1] * shade + (hi_col[1] - mid_col[1]) * spec_band * 0.88
            b = mid_col[2] * shade + (hi_col[2] - mid_col[2]) * spec_band * 0.88

            dark_f = clamp(0.74 * knot_ao + 0.58 * hairline_ao, 0.0, 0.90)
            r = r * (1.0 - dark_f) + root_col[0] * dark_f
            g = g * (1.0 - dark_f) + root_col[1] * dark_f
            b = b * (1.0 - dark_f) + root_col[2] * dark_f

            checker = 1.045 if ((x + y) & 1) == 0 else 0.955
            jitter = 0.94 + 0.12 * h2(x, y)
            r = clamp(r * checker * jitter, shadow_col[0], 0.48)
            g = clamp(g * checker * jitter, shadow_col[1], 0.36)
            b = clamp(b * checker * jitter, shadow_col[2], 0.26)

            idx = (y * w + x) * 4
            px[idx : idx + 4] = [r, g, b, 1.0]

    img = bpy.data.images.new("coque_samurai_masculino_128", width=w, height=h, alpha=True)
    img.pixels[:] = px
    img.filepath_raw = str(png_path)
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
        x0, y0 = int(round((u0 % 1.0) * (size - 1))), int(round(clamp(v0) * (size - 1)))
        x1, y1 = int(round((u1 % 1.0) * (size - 1))), int(round(clamp(v1) * (size - 1)))
        if abs(x1 - x0) > size // 2:
            return
        steps = max(abs(x1 - x0), abs(y1 - y0), 1)
        for s in range(steps + 1):
            t = s / steps
            put_px(int(round(x0 + (x1 - x0) * t)), int(round(y0 + (y1 - y0) * t)), (0.95, 0.82, 0.35, 0.90))

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


def build_head_bvh(body_obj) -> BVHTree:
    """BVHTree of chartest_body's Basis head+neck surface for exact skull raycasting."""
    bm = bmesh.new()
    bm.from_mesh(body_obj.data)
    if body_obj.data.shape_keys:
        basis = body_obj.data.shape_keys.key_blocks[0].data
        for i, v in enumerate(bm.verts):
            v.co = basis[i].co.copy()
    del_verts = [v for v in bm.verts if v.co.z < 1.53]
    bmesh.ops.delete(bm, geom=del_verts, context="VERTS")
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    bvh = BVHTree.FromBMesh(bm)
    bm.free()
    return bvh


def skull_raycast(bvh: BVHTree, origin: Vector, direction: Vector, fallback_r: float = 0.095):
    d = direction.normalized()
    loc, norm, _, _ = bvh.ray_cast(origin, d, 0.35)
    if loc is not None:
        return loc, (norm if norm.dot(d) > 0 else -norm)
    far = origin + d * 0.30
    loc2, norm2, _, _ = bvh.ray_cast(far, -d, 0.35)
    if loc2 is not None:
        return loc2, (norm2 if norm2.dot(d) > 0 else -norm2)
    return origin + d * fallback_r, d


def samurai_hairline_z(phi: float) -> float:
    """Pulled-back masculine warrior hairline Z(phi) around C=(0, -0.075, 1.690)."""
    deg = math.degrees(phi)
    adeg = abs(deg)

    # 1. Strong pulled-back masculine forehead with subtle center widow's peak (1.742m)
    #    and clean masculine temporal corners (+-31 deg at 1.754m)
    z = 1.747
    z -= 0.005 * bell(deg, 0.0, 14.0)      # subtle masculine widow's peak at center forehead
    z += 0.008 * bell(adeg, 31.0, 13.0)    # natural masculine temple corners ("entradas suaves")
    z -= 0.005 * bell(adeg, 52.0, 10.0)    # crisp temporal point ("ponta da têmpora")

    # 2. Sleek temple descent into masculine sideburn ("costeleta") in front of ear
    temple_drop = smoothstep(48.0, 72.0, adeg)
    z -= 0.029 * temple_drop

    sideburn = bell(adeg, 81.0, 10.0)
    z -= 0.036 * sideburn  # dips cleanly to z ~ 1.682m in front of tragus

    # 3. Snug smooth Ear Cutout Arch (ear spans adeg 95.9..108.9, z_ear_top = 1.7065m)
    ear_arch = smoothstep(87.0, 94.0, adeg) * (1.0 - smoothstep(108.5, 112.0, adeg))
    z = z * (1.0 - ear_arch) + (1.710 + 0.004 * bell(adeg, 101.0, 9.0)) * ear_arch

    # 4. Immediate post-ear drop (fills behind-ear skin!) into taut upward-pulled nape (z ~ 1.619m)
    post_ear_drop = smoothstep(109.0, 125.0, adeg)
    nape_settle = smoothstep(123.0, 158.0, adeg)
    # Subtle upward-pulled M/arch tension at center nape (z=1.622m) where strands pull up to the bun!
    z_nape = 1.623 * (1.0 - nape_settle) + (1.618 + 0.005 * bell(adeg, 180.0, 22.0)) * nape_settle
    z = z * (1.0 - post_ear_drop) + z_nape * post_ear_drop

    return z


def build_samurai_bun_mesh(body_obj) -> bpy.types.Object:
    """Sculpt taut pulled-back cranial hair + cord tie wrap + folded samurai bun + short tail fan."""
    bvh = build_head_bvh(body_obj)
    C = Vector((0.0, -0.075, 1.690))

    # Convergence pole on upper-back crown (~37 deg back from apex): all hair pulls straight here!
    tie_dir = Vector((0.0, -0.60, 0.80)).normalized()
    tie_skull, tie_n = skull_raycast(bvh, C, tie_dir)
    bun_axis = (tie_dir * 0.55 + tie_n * 0.45).normalized()
    # Slightly lift bun_axis upward (~45 deg) so the bun projects proudly above the crown in Front View!
    bun_axis = Vector((0.0, -0.71, 0.704)).normalized()

    tie_root_pt = tie_skull + bun_axis * 0.0125

    N_AZ = 40
    s_rings = [0.14, 0.30, 0.48, 0.66, 0.84, 1.00]

    bm = bmesh.new()
    uv_lay = bm.loops.layers.uv.new("UVMap")

    phis = []
    for k in range(N_AZ):
        raw_phi = -math.pi + (2.0 * math.pi * k) / N_AZ
        raw_deg = math.degrees(raw_phi)
        sign = 1.0 if raw_deg >= 0.0 else -1.0
        # Align +-108 deg column to +-109.6 deg right behind ear helix tip
        warped_deg = raw_deg + sign * 1.6 * bell(abs(raw_deg), 108.0, 8.0)
        phis.append(math.radians(warped_deg))

    hl_dirs = []
    for phi in phis:
        zh = samurai_hairline_z(phi)
        axis_pt = Vector((0.0, -0.075, clamp(zh, 1.615, 1.775)))
        horiz_dir = Vector((math.sin(phi), math.cos(phi), 0.0)).normalized()
        p_hit, _ = skull_raycast(bvh, axis_pt, horiz_dir, fallback_r=0.082)
        p_target = Vector((p_hit.x, p_hit.y, zh))
        hl_dirs.append((p_target - C).normalized())

    # =========================================================================
    # 1. TAUT PULLED-BACK CRANIAL CAP CONVERGING INTO THE TIE ROOT
    # =========================================================================
    v_tie_center = bm.verts.new(tie_root_pt)
    ring_verts = []
    inner_lip_verts = []

    for r_idx, s in enumerate(s_rings):
        row = []
        for k, phi in enumerate(phis):
            deg = math.degrees(phi)
            adeg = abs(deg)

            base_dir = slerp_dir(tie_dir, hl_dirs[k], s)
            p_sk, n_sk = skull_raycast(bvh, C, base_dir)
            out_dir = (base_dir * 0.58 + n_sk * 0.42).normalized()

            topness = clamp((base_dir.z + 0.05) / 0.95, 0.0, 1.0)
            frontness = smoothstep(80.0, 0.0, adeg)
            sideness = bell(adeg, 94.0, 36.0)

            # Taut, disciplined pulled-back profile: sleek on sides (~9.5mm), firm across top crown (~14.5mm),
            # gathering slightly thicker (~15mm) right around the tie collar (s=0.14)!
            knot_gather = bell(s, 0.14, 0.22) * 0.0035
            thick = 0.0095 + 0.0052 * (topness ** 0.9) + knot_gather
            thick -= 0.0018 * sideness * smoothstep(0.35, 1.0, s)

            # Sculpted 3D pulled-strand tension ridges radiating directly into the bun!
            tension_ridges = math.cos(phi * 10.0) * 0.0016 * math.sin(s * math.pi * 0.92)
            thick += tension_ridges

            if r_idx == len(s_rings) - 1:
                # Crisp masculine hairline bevel (~4.5mm on front hairline, ~3.2mm on sideburns & nape)
                edge_keep = 0.34 + 0.16 * frontness
                thick *= edge_keep
                if 94.0 <= adeg <= 109.8:
                    p_sk.z = max(p_sk.z, 1.710)
            elif r_idx == len(s_rings) - 2:
                thick *= 0.88 + 0.06 * frontness

            thick = max(0.0030, thick)
            p_hair = p_sk + out_dir * thick
            row.append(bm.verts.new(p_hair))

            if r_idx == len(s_rings) - 1:
                lip_pt = p_sk - out_dir * 0.0018 + Vector((0.0, 0.0, 0.0015))
                if 94.0 <= adeg <= 109.8:
                    lip_pt.z = max(lip_pt.z, 1.7095)
                inner_lip_verts.append(bm.verts.new(lip_pt))

        ring_verts.append(row)

    def u_of(k_idx):
        return k_idx / N_AZ

    def v_of_cap(r_idx):
        # Knot root = 0.97, outer hairline = 0.19 (strictly inside hair region V >= 0.18!)
        s = s_rings[r_idx]
        return 0.97 - 0.78 * s

    r0 = ring_verts[0]
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((v_tie_center, r0[k], r0[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (0.5 * (u0 + u1), 0.985)
        f.loops[1][uv_lay].uv = (u0, v_of_cap(0))
        f.loops[2][uv_lay].uv = (u1, v_of_cap(0))

    for r_idx in range(len(s_rings) - 1):
        ra, rb = ring_verts[r_idx], ring_verts[r_idx + 1]
        va, vb = v_of_cap(r_idx), v_of_cap(r_idx + 1)
        for k in range(N_AZ):
            kn = (k + 1) % N_AZ
            f = bm.faces.new((ra[k], rb[k], rb[kn], ra[kn]))
            u0, u1 = u_of(k), u_of(k + 1)
            f.loops[0][uv_lay].uv = (u0, va)
            f.loops[1][uv_lay].uv = (u0, vb)
            f.loops[2][uv_lay].uv = (u1, vb)
            f.loops[3][uv_lay].uv = (u1, va)

    # Watertight hairline bevel + dark interior dome
    r_last = ring_verts[-1]
    v_last = v_of_cap(len(s_rings) - 1)
    v_inner_crown = bm.verts.new(tie_skull - tie_n * 0.0025)
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((r_last[k], inner_lip_verts[k], inner_lip_verts[kn], r_last[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (u0, v_last)
        f.loops[1][uv_lay].uv = (u0, 0.18)
        f.loops[2][uv_lay].uv = (u1, 0.18)
        f.loops[3][uv_lay].uv = (u1, v_last)

        f_in = bm.faces.new((inner_lip_verts[kn], inner_lip_verts[k], v_inner_crown))
        for lp in f_in.loops:
            lp[uv_lay].uv = (0.5, 0.03)

    # =========================================================================
    # 2. ORTHONORMAL FRAME FOR CORD TIE + FOLDED SAMURAI BUN
    # =========================================================================
    # bun_x = +X (right), bun_z = bun_axis (backward-upward ~45 deg), bun_y = crest-upward perpendicular!
    bun_x = Vector((1.0, 0.0, 0.0))
    bun_z = bun_axis.normalized()
    bun_y = bun_z.cross(bun_x).normalized()  # points forward-and-up (+0.704 Y, +0.710 Z)

    N_BUN = 16
    # Anchor base slightly inside the cap surface so the cord wrap sits flush with zero gap
    base_origin = tie_skull + bun_z * 0.0085

    # --- 2A. Dark Leather / Cord Tie Collar (UV mapped to V in [0.085 .. 0.160]) ---
    # 4 axial stations forming a snug double-wrapped warrior tie band
    cord_stations = [
        # (axial_dist, rx, ry, v_uv)
        (0.000, 0.0235, 0.0215, 0.085),
        (0.004, 0.0258, 0.0238, 0.110),
        (0.013, 0.0252, 0.0232, 0.140),
        (0.017, 0.0220, 0.0200, 0.162),
    ]
    cord_rings = []
    for ax_d, rx, ry, _ in cord_stations:
        c_pt = base_origin + bun_z * ax_d
        ring = []
        for i in range(N_BUN):
            th = (2.0 * math.pi * i) / N_BUN
            p = c_pt + bun_x * (rx * math.cos(th)) + bun_y * (ry * math.sin(th))
            ring.append(bm.verts.new(p))
        cord_rings.append(ring)

    # Cap inner base of cord collar inside hair cap
    v_cord_base = bm.verts.new(base_origin)
    for i in range(N_BUN):
        inext = (i + 1) % N_BUN
        fb = bm.faces.new((v_cord_base, cord_rings[0][inext], cord_rings[0][i]))
        for lp in fb.loops:
            lp[uv_lay].uv = (0.5, 0.09)

    for s_i in range(len(cord_stations) - 1):
        ra, rb = cord_rings[s_i], cord_rings[s_i + 1]
        va, vb = cord_stations[s_i][3], cord_stations[s_i + 1][3]
        for i in range(N_BUN):
            inext = (i + 1) % N_BUN
            u0, u1 = i / N_BUN, (i + 1) / N_BUN
            f = bm.faces.new((ra[i], rb[i], rb[inext], ra[inext]))
            f.loops[0][uv_lay].uv = (u0, va)
            f.loops[1][uv_lay].uv = (u0, vb)
            f.loops[2][uv_lay].uv = (u1, vb)
            f.loops[3][uv_lay].uv = (u1, va)

    # --- 2B. Sculpted Folded Masculine Samurai Bun (Oblong Chiseled Topknot Loop) ---
    # Rises proudly from inside the cord tie (ax=0.013m) out to ax=0.068m, curving slightly forward-up
    # along +bun_y so it crowns the warrior silhouette cleanly from Front, Side, and Back!
    bun_stations = [
        # (axial_dist, y_crest_lift, rx_width, ry_height, v_uv)
        (0.014,  0.0000, 0.0200, 0.0185, 0.94),
        (0.026, +0.0030, 0.0310, 0.0278, 0.80),
        (0.044, +0.0062, 0.0342, 0.0310, 0.62),
        (0.060, +0.0072, 0.0298, 0.0266, 0.44),
        (0.072, +0.0055, 0.0185, 0.0162, 0.28),
    ]
    bun_rings = []
    for s_idx, (ax_d, y_lift, rx, ry, _) in enumerate(bun_stations):
        c_pt = base_origin + bun_z * ax_d + bun_y * y_lift
        ring = []
        mid_f = math.sin((s_idx / len(bun_stations)) * math.pi)
        for i in range(N_BUN):
            th = (2.0 * math.pi * i) / N_BUN
            ct, st = math.cos(th), math.sin(th)
            # Chiseled warrior bun shaping:
            # - Slightly squared-off masculine shoulders (+0.08 * cos(4*th))
            # - Subtle folded-hair loop tuck crease along underside (st < 0) & center fold groove (ct ~ 0)
            squareness = 1.0 + 0.055 * math.cos(4.0 * th)
            fold_groove = 1.0 - 0.10 * (1.0 - abs(ct)) ** 2.2 * smoothstep(0.0, -0.8, st) * mid_f
            strand_facet = 1.0 + 0.045 * math.cos(8.0 * th) * mid_f
            scale_r = squareness * fold_groove * strand_facet

            p = c_pt + bun_x * (rx * ct * scale_r) + bun_y * (ry * st * scale_r)
            ring.append(bm.verts.new(p))
        bun_rings.append(ring)

    # Cap base inside cord collar & folded loop apex at tip (tucked slightly inward like folded hair!)
    v_bun_root = bm.verts.new(base_origin + bun_z * 0.012)
    v_bun_apex = bm.verts.new(base_origin + bun_z * 0.0745 + bun_y * 0.0045)

    for i in range(N_BUN):
        inext = (i + 1) % N_BUN
        fr = bm.faces.new((v_bun_root, bun_rings[0][inext], bun_rings[0][i]))
        for lp in fr.loops:
            lp[uv_lay].uv = (0.5, 0.95)

    for s_i in range(len(bun_stations) - 1):
        ra, rb = bun_rings[s_i], bun_rings[s_i + 1]
        va, vb = bun_stations[s_i][4], bun_stations[s_i + 1][4]
        for i in range(N_BUN):
            inext = (i + 1) % N_BUN
            u0, u1 = i / N_BUN, (i + 1) / N_BUN
            f = bm.faces.new((ra[i], rb[i], rb[inext], ra[inext]))
            f.loops[0][uv_lay].uv = (u0, va)
            f.loops[1][uv_lay].uv = (u0, vb)
            f.loops[2][uv_lay].uv = (u1, vb)
            f.loops[3][uv_lay].uv = (u1, va)

    last_ring = bun_rings[-1]
    v_last_bun = bun_stations[-1][4]
    for i in range(N_BUN):
        inext = (i + 1) % N_BUN
        u0, u1 = i / N_BUN, (i + 1) / N_BUN
        ft = bm.faces.new((last_ring[i], v_bun_apex, last_ring[inext]))
        ft.loops[0][uv_lay].uv = (u0, v_last_bun)
        ft.loops[1][uv_lay].uv = (0.5 * (u0 + u1), 0.21)
        ft.loops[2][uv_lay].uv = (u1, v_last_bun)

    # =========================================================================
    # 3. CRISP SHORT BRUSHED WARRIOR TAIL TUFT EMERGING UNDER THE FOLDED BUN
    # =========================================================================
    def add_tail_tuft(spine_pts, out_nrms, widths, thicknesses, u_c=0.50):
        n_seg = len(spine_pts)
        sec = []
        for idx, pt in enumerate(spine_pts):
            no = out_nrms[idx].normalized()
            tang = (spine_pts[idx + 1] - pt).normalized() if idx < n_seg - 1 else (pt - spine_pts[idx - 1]).normalized()
            side = tang.cross(no).normalized()
            wh = widths[idx] * 0.5
            thk = thicknesses[idx]
            v_l = bm.verts.new(pt - side * wh - no * (thk * 0.45))
            v_r = bm.verts.new(pt + side * wh - no * (thk * 0.45))
            v_t = bm.verts.new(pt + no * thk)
            sec.append((v_l, v_t, v_r))

        rl, rt, rr = sec[0]
        f0 = bm.faces.new((rl, rr, rt))
        for lp in f0.loops:
            lp[uv_lay].uv = (u_c, 0.90)

        tl, tt, tr = sec[-1]
        ftip = bm.faces.new((tl, tt, tr))
        ftip.loops[0][uv_lay].uv = (u_c - 0.05, 0.20)
        ftip.loops[1][uv_lay].uv = (u_c, 0.21)
        ftip.loops[2][uv_lay].uv = (u_c + 0.05, 0.20)

        du = 0.08
        for s_i in range(n_seg - 1):
            v0 = 0.90 - 0.70 * (s_i / (n_seg - 1))
            v1 = 0.90 - 0.70 * ((s_i + 1) / (n_seg - 1))
            cur, nxt = sec[s_i], sec[s_i + 1]

            f1 = bm.faces.new((cur[0], nxt[0], nxt[1], cur[1]))
            f1.loops[0][uv_lay].uv = (u_c - du, v0)
            f1.loops[1][uv_lay].uv = (u_c - du, v1)
            f1.loops[2][uv_lay].uv = (u_c, v1)
            f1.loops[3][uv_lay].uv = (u_c, v0)

            f2 = bm.faces.new((cur[1], nxt[1], nxt[2], cur[2]))
            f2.loops[0][uv_lay].uv = (u_c, v0)
            f2.loops[1][uv_lay].uv = (u_c, v1)
            f2.loops[2][uv_lay].uv = (u_c + du, v1)
            f2.loops[3][uv_lay].uv = (u_c + du, v0)

            f3 = bm.faces.new((cur[2], nxt[2], nxt[0], cur[0]))
            f3.loops[0][uv_lay].uv = (u_c + du, v0)
            f3.loops[1][uv_lay].uv = (u_c + du, v1)
            f3.loops[2][uv_lay].uv = (u_c - du, v1)
            f3.loops[3][uv_lay].uv = (u_c - du, v0)

    # Short brushed tail fan (~36mm long) emerging snugly from lower edge of cord wrap
    tail_base = base_origin + bun_z * 0.014 - bun_y * 0.011
    add_tail_tuft(
        [
            tail_base,
            tail_base + Vector((0.0, -0.016, -0.010)),
            tail_base + Vector((0.0, -0.028, -0.024)),
            tail_base + Vector((0.0, -0.034, -0.038)),
        ],
        [
            Vector((0.0, -0.88, 0.48)),
            Vector((0.0, -0.92, 0.38)),
            Vector((0.0, -0.95, 0.30)),
            Vector((0.0, -0.96, 0.25)),
        ],
        widths=[0.032, 0.038, 0.034, 0.020],
        thicknesses=[0.0075, 0.0092, 0.0072, 0.0032],
        u_c=0.50,
    )

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])

    mesh = bpy.data.meshes.new("CoqueSamuraiMasculino")
    bm.to_mesh(mesh)
    bm.free()

    for poly in mesh.polygons:
        poly.use_smooth = False

    hair_obj = bpy.data.objects.new("CoqueSamuraiMasculino", mesh)
    bpy.context.scene.collection.objects.link(hair_obj)
    return hair_obj


def verify_samurai_hair_fit(hair_obj):
    """Verify triangle budget, ear clearance, brow clearance, and bun silhouette height."""
    bm = bmesh.new()
    bm.from_mesh(hair_obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    tri_count = len(bm.faces)
    verts = [v.co.copy() for v in bm.verts]
    bm.free()

    ear_zone = [p for p in verts if -0.1028 <= p.y <= -0.0825 and abs(p.x) > 0.078]
    min_ear_z = min((p.z for p in ear_zone), default=9.9)
    behind_ear_zone = [p for p in verts if -0.132 <= p.y <= -0.106 and abs(p.x) > 0.060]
    min_behind_ear_z = min((p.z for p in behind_ear_zone), default=9.9)
    brow_zone = [p for p in verts if p.y > 0.032 and abs(p.x) < 0.045]
    min_brow_z = min((p.z for p in brow_zone), default=9.9)
    max_bun_z = max(p.z for p in verts)
    min_bun_y = min(p.y for p in verts)

    print(f"[samurai-verify] triangles={tri_count}, verts={len(verts)}")
    print(f"[samurai-verify] ear_arch_min_z={min_ear_z:.4f}m (clearance={(min_ear_z - 1.7065)*1000:.1f}mm)")
    print(f"[samurai-verify] behind_ear_min_z={min_behind_ear_z:.4f}m")
    print(f"[samurai-verify] forehead_min_z={min_brow_z:.4f}m (clearance={(min_brow_z - 1.718)*1000:.1f}mm)")
    print(f"[samurai-verify] bun_top_z={max_bun_z:.4f}m, bun_back_y={min_bun_y:.4f}m")
    assert min_ear_z >= 1.7085, f"Hair encroaches on ear! min_ear_z={min_ear_z}"
    assert min_brow_z >= 1.734, f"Hair hangs too low over brow! min_brow_z={min_brow_z}"
    assert max_bun_z >= 1.832, f"Samurai bun not proud enough in front silhouette! max_bun_z={max_bun_z}"
    return tri_count


def export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path: pathlib.Path):
    """Export CoqueSamuraiMasculino.glb with head_socket pivot & embedded 128x128 PNG."""
    head_pivot = arm_obj.data.bones["head"].head_local.copy()

    pivot_empty = bpy.data.objects.new("head_socket", None)
    bpy.context.scene.collection.objects.link(pivot_empty)
    pivot_empty.location = head_pivot

    export_copy = hair_obj.copy()
    export_copy.data = hair_obj.data.copy()
    export_copy.name = "CoqueSamuraiMasculino"
    export_copy.data.transform(Matrix.Translation(-head_pivot))
    export_copy.parent = pivot_empty
    export_copy.matrix_parent_inverse = Matrix.Identity(4)
    export_copy.location = (0.0, 0.0, 0.0)
    bpy.context.scene.collection.objects.link(export_copy)

    mat = bpy.data.materials.new("coque_samurai_masculino")
    mat.use_nodes = True
    mat.use_backface_culling = False
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.86
    bsdf.inputs["Metallic"].default_value = 0.0
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.14
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = tex_img
    tex.interpolation = "Closest"
    links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])

    export_copy.data.materials.clear()
    export_copy.data.materials.append(mat)
    hair_obj.data.materials.clear()
    hair_obj.data.materials.append(mat)

    bpy.ops.object.select_all(action="DESELECT")
    pivot_empty.select_set(True)
    export_copy.select_set(True)
    bpy.context.view_layer.objects.active = pivot_empty

    bpy.ops.export_scene.gltf(
        filepath=str(glb_path),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=False,
        export_texcoords=True,
        export_normals=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
    )

    tmp_mesh = export_copy.data
    bpy.data.objects.remove(export_copy, do_unlink=True)
    bpy.data.objects.remove(pivot_empty, do_unlink=True)
    bpy.data.meshes.remove(tmp_mesh)

    raw = glb_path.read_bytes()
    magic, version, total_len = struct.unpack_from("<4sII", raw, 0)
    assert magic == b"glTF" and version == 2 and total_len == len(raw)
    jlen = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20 : 20 + jlen])
    print(f"[glb-verify] {glb_path.name} ({len(raw)} bytes), images={len(doc.get('images', []))}, meshes={len(doc.get('meshes', []))}")


def render_samurai_previews(hair_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render 4-angle head close-ups + full-body & isolated 3D piece."""
    scene = bpy.context.scene
    for t in arm_obj.animation_data.nla_tracks:
        t.mute = True
    arm_obj.animation_data.action = None

    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"
    if hasattr(scene, "eevee"):
        if hasattr(scene.eevee, "samples"):
            scene.eevee.samples = 12
        elif hasattr(scene.eevee, "taa_render_samples"):
            scene.eevee.taa_render_samples = 12

    world = bpy.data.worlds.new("SamuraiPreviewWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.055, 0.064, 0.080, 1.0)
    bg.inputs[1].default_value = 1.0

    lights = []
    for lname, loc, energy, col in [
        ("SKey", (-2.0, 3.6, 3.4), 4.4, (1.0, 0.97, 0.92)),
        ("SFill", (2.8, 2.6, 2.2), 2.2, (0.82, 0.88, 1.0)),
        ("SRim", (0.2, -3.5, 2.9), 3.0, (0.95, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy = energy
        ldata.color = col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.72)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("SamuraiCam")
    cam = bpy.data.objects.new("SamuraiCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    depsgraph = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = True
    arm_obj.hide_render = True
    hair_obj.hide_render = True

    def spawn_pair(x_pos, yaw_deg, include_body=True):
        arm_obj["height_m"] = 1.82
        arm_obj["is_female"] = False
        arm_obj.update_tag()
        body_obj.update_tag()
        depsgraph.update()
        objs = []
        rot = (0.0, 0.0, math.radians(yaw_deg))
        if include_body:
            ev_b = body_obj.evaluated_get(depsgraph)
            mb = bpy.data.meshes.new_from_object(ev_b, preserve_all_data_layers=True, depsgraph=depsgraph)
            ob = bpy.data.objects.new("SnapBody", mb)
            scene.collection.objects.link(ob)
            ob.location = (x_pos, 0.0, 0.0)
            ob.rotation_euler = rot
            objs.append(ob)

        mh = hair_obj.data.copy()
        oh = bpy.data.objects.new("SnapHair", mh)
        scene.collection.objects.link(oh)
        oh.location = (x_pos, 0.0, 0.0)
        oh.rotation_euler = rot
        objs.append(oh)
        return objs

    def cleanup_objs(objs):
        for o in objs:
            m = o.data
            bpy.data.objects.remove(o, do_unlink=True)
            if m.users == 0:
                bpy.data.meshes.remove(m)

    # --- SHEET 1: 4-Angle Head Close-up (Front 0°, 3/4 Front +38°, Pure Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(0.56, 0.0), (0.19, 38.0), (-0.19, -90.0), (-0.58, -142.0)]:
        snaps.extend(spawn_pair(x_p, yaw, include_body=True))

    scene.render.resolution_x = 1640
    scene.render.resolution_y = 700
    cam.location = (-0.01, 3.0, 1.73)
    cam.rotation_euler = (Vector((-0.01, 0.0, 1.71)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 1.56
    scene.render.filepath = str(out_dir / "preview_samurai_bun_angles.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    # --- SHEET 2: Full-Body Front/Back + Isolated 3D Samurai Bun Piece
    snaps = []
    snaps.extend(spawn_pair(1.55, -26.0, include_body=True))
    snaps.extend(spawn_pair(0.48, -135.0, include_body=True))
    for x_p, yaw in [(-0.60, 42.0), (-1.56, -125.0)]:
        iso = spawn_pair(x_p, yaw, include_body=False)
        for o in iso:
            o.scale = (2.5, 2.5, 2.5)
            o.location = (x_p, 0.0, 1.00 - 1.73 * 2.5)
        snaps.extend(iso)

    scene.render.resolution_x = 1580
    scene.render.resolution_y = 800
    cam.location = (0.0, 5.0, 0.96)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.50
    scene.render.filepath = str(out_dir / "preview_samurai_bun_full_and_iso.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    body_obj.hide_render = False
    arm_obj.hide_render = False
    hair_obj.hide_render = False
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
    arm_obj["is_female"] = False
    arm_obj["height_m"] = 1.82

    hair_obj = build_samurai_bun_mesh(body_obj)
    verify_samurai_hair_fit(hair_obj)

    tex_png = OUT_DIR / "coque_samurai_masculino_128.png"
    uv_png = OUT_DIR / "coque_samurai_masculino_uv.png"
    glb_path = OUT_DIR / "CoqueSamuraiMasculino.glb"
    blend_out = OUT_DIR / "coque_samurai_masculino.blend"

    tex_img = generate_psx_samurai_hair_texture(tex_png)
    export_uv_overlay(hair_obj, uv_png)
    export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path)

    hair_obj.parent = arm_obj
    hair_obj.parent_type = "BONE"
    hair_obj.parent_bone = "head"
    head_bone = arm_obj.pose.bones["head"]
    hair_obj.matrix_parent_inverse = (arm_obj.matrix_world @ head_bone.matrix).inverted()

    bpy.ops.wm.save_as_mainfile(filepath=str(blend_out))
    render_samurai_previews(hair_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CoqueSamuraiMasculino built & rendered successfully!")


if __name__ == "__main__":
    main()
