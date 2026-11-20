"""Build Grimora low-poly PSX Loose Female Bun (CoqueSoltoFeminino) fitted to chartest.

Generates in the conversation staging directory (prior to user approval):
- CoqueSoltoFeminino.glb                    (embedded 128x128 PSX wavy hair texture, head_socket hierarchy)
- coque_solto_feminino_128.png              (128x128 dark-fantasy PSX soft wavy hair texture)
- coque_solto_feminino_uv.png               (128x128 transparent UV overlay)
- coque_solto_feminino.blend                (authoring scene parented to chartest_rig -> head)
- preview_female_bun_angles.png             (4-angle close-up on Female chartest: Front, 3/4, Side, 3/4 Back)
- preview_female_bun_full_and_iso.png       (Full-body Female front/back + Male check + isolated 3D piece)
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
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\hair_loose_female_bun"
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


def generate_psx_wavy_bun_texture(png_path: pathlib.Path, w: int = 128, h: int = 128):
    """Author a soft, wavy 128x128 PSX dark-fantasy warm chestnut hair texture.

    UV layout convention:
    - V in [0.10 .. 0.98]: softly waved hair strands flowing from hairline/tips (V=0.10) into the
      gathered bun & spiral coils (V=0.98).
    - V in [0.00 .. 0.08]: deep warm dark interior scalp shadow strip.
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

    # Rich warm chestnut-umber dark-fantasy palette with soft romantic sheen
    root_col = (0.064, 0.043, 0.031)
    mid_col = (0.222, 0.148, 0.098)
    hi_col = (0.372, 0.258, 0.174)
    shadow_col = (0.042, 0.029, 0.021)

    for y in range(h):
        v = (y + 0.5) / h
        if v < 0.080:
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

        vh = (v - 0.080) / (1.0 - 0.080)
        vq = (int(y // 2) + 0.5) / (h / 2.0)

        for x in range(w):
            u_raw = (x + 0.5) / w
            # Soft romantic S-wave in the strands so the hair reads as loose & wavy rather than stiff!
            wave = math.sin(vh * 8.5 + periodic_noise_1d(u_raw, 10, 9) * 3.5) * 0.015
            wave += (periodic_noise_1d(u_raw, 16, int(y // 12) + 27) - 0.5) * 0.012
            u = (u_raw + wave) % 1.0

            broad_lock = periodic_noise_1d(u, 12, 7)
            med_lock = periodic_noise_1d(u, 28, 21)
            fine_strand = periodic_noise_1d(u, 56, 43 + (y // 18))
            micro_stroke = h2(x, y // 3)

            strand = (
                0.38 * broad_lock
                + 0.32 * med_lock
                + 0.18 * fine_strand
                + 0.12 * micro_stroke
            )

            off = (periodic_noise_1d(u, 14, 71) - 0.5) * 0.14
            spec1 = bell(vh, 0.66 + off, 0.20)
            spec2 = bell(vh, 0.34 - off * 0.6, 0.16)
            spec_band = (spec1 + 0.65 * spec2) * smoothstep(0.38, 0.76, strand)

            gather_ao = smoothstep(0.86, 0.99, vh)
            tip_ao = smoothstep(0.16, 0.01, vh)
            part_shadow = bell(u_raw, 0.494, 0.022) * smoothstep(0.18, 0.62, vh) * 0.40

            shade = 0.62 + 0.74 * strand
            r = mid_col[0] * shade + (hi_col[0] - mid_col[0]) * spec_band * 0.88
            g = mid_col[1] * shade + (hi_col[1] - mid_col[1]) * spec_band * 0.88
            b = mid_col[2] * shade + (hi_col[2] - mid_col[2]) * spec_band * 0.88

            dark_f = clamp(0.70 * gather_ao + 0.55 * tip_ao + part_shadow, 0.0, 0.90)
            r = r * (1.0 - dark_f) + root_col[0] * dark_f
            g = g * (1.0 - dark_f) + root_col[1] * dark_f
            b = b * (1.0 - dark_f) + root_col[2] * dark_f

            checker = 1.045 if ((x + y) & 1) == 0 else 0.955
            jitter = 0.94 + 0.12 * h2(x, y)
            r = clamp(r * checker * jitter, shadow_col[0], 0.49)
            g = clamp(g * checker * jitter, shadow_col[1], 0.37)
            b = clamp(b * checker * jitter, shadow_col[2], 0.27)

            idx = (y * w + x) * 4
            px[idx : idx + 4] = [r, g, b, 1.0]

    img = bpy.data.images.new("coque_solto_feminino_128", width=w, height=h, alpha=True)
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
    """BVHTree of chartest_body's Basis head+neck surface."""
    bm = bmesh.new()
    bm.from_mesh(body_obj.data)
    if body_obj.data.shape_keys:
        basis = body_obj.data.shape_keys.key_blocks[0].data
        for i, v in enumerate(bm.verts):
            v.co = basis[i].co.copy()
    del_verts = [v for v in bm.verts if v.co.z < 1.49]
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


def loose_feminine_hairline_z(phi: float) -> float:
    """Soft, rounded feminine center-parted curtain hairline + relaxed low-side sweep."""
    deg = math.degrees(phi)
    adeg = abs(deg)

    # 1. Soft center-parted curtain bangs (~1.731m..1.755m): NO masculine temple recessions!
    #    Instead, a gentle inverted-V center part (-2 deg) sweeping smoothly down across the temples!
    z = 1.737
    z += 0.019 * bell(deg, -2.0, 17.0)
    z -= 0.006 * bell(adeg, 34.0, 18.0)

    # 2. Soft relaxed side-sweep grazing down across temples toward the ear
    temple_drop = smoothstep(42.0, 76.0, adeg)
    z -= 0.030 * temple_drop

    # Soft feminine side-wisp base in front of ear (blends into the loose jaw-framing tendrils!)
    cheek_sweep = bell(adeg, 79.0, 10.5)
    z -= 0.032 * cheek_sweep

    # 3. Snug, gentle Ear Arch (clears ear helix at z~1.711m, then immediately drops into a soft
    #    relaxed pouch behind the ear!)
    ear_arch = smoothstep(87.0, 94.0, adeg) * (1.0 - smoothstep(108.5, 112.0, adeg))
    z = z * (1.0 - ear_arch) + (1.710 + 0.003 * bell(adeg, 101.0, 9.0)) * ear_arch

    # 4. Soft low-draped feminine nape & post-ear pouch (z ~ 1.616m..1.622m)
    post_ear_drop = smoothstep(109.0, 124.0, adeg)
    nape_settle = smoothstep(122.0, 158.0, adeg)
    z_nape = 1.620 * (1.0 - nape_settle) + (1.615 + 0.004 * bell(adeg, 180.0, 24.0)) * nape_settle
    z = z * (1.0 - post_ear_drop) + z_nape * post_ear_drop

    return z


def build_loose_female_bun_mesh(body_obj) -> bpy.types.Object:
    """Sculpt airy center-parted crown + relaxed side pouches + plush coiled bun + jaw tendrils."""
    bvh = build_head_bvh(body_obj)
    C = Vector((0.0, -0.075, 1.690))

    # Relaxed mid-low occipital bun anchor (~27 deg above horizontal on back of head — lower & softer
    # than the high samurai topknot, classic romantic/loose feminine chignon placement!)
    bun_dir = Vector((0.0, -0.89, 0.455)).normalized()
    bun_skull, bun_n = skull_raycast(bvh, C, bun_dir)
    bun_axis = Vector((0.0, -0.925, 0.380)).normalized()  # ~22 deg relaxed angle

    gather_root_pt = bun_skull + bun_axis * 0.018

    N_AZ = 40
    s_rings = [0.15, 0.32, 0.50, 0.68, 0.85, 1.00]

    bm = bmesh.new()
    uv_lay = bm.loops.layers.uv.new("UVMap")

    phis = []
    for k in range(N_AZ):
        raw_phi = -math.pi + (2.0 * math.pi * k) / N_AZ
        raw_deg = math.degrees(raw_phi)
        sign = 1.0 if raw_deg >= 0.0 else -1.0
        warped_deg = raw_deg + sign * 1.6 * bell(abs(raw_deg), 108.0, 8.0)
        phis.append(math.radians(warped_deg))

    hl_dirs = []
    for phi in phis:
        zh = loose_feminine_hairline_z(phi)
        axis_pt = Vector((0.0, -0.075, clamp(zh, 1.612, 1.775)))
        horiz_dir = Vector((math.sin(phi), math.cos(phi), 0.0)).normalized()
        p_hit, _ = skull_raycast(bvh, axis_pt, horiz_dir, fallback_r=0.082)
        p_target = Vector((p_hit.x, p_hit.y, zh))
        hl_dirs.append((p_target - C).normalized())

    # =========================================================================
    # 1. AIRY CENTER-PARTED CAP WITH RELAXED LOOSE SIDE-SWEEP POUCHES
    # =========================================================================
    v_gather_center = bm.verts.new(gather_root_pt)
    ring_verts = []
    inner_lip_verts = []

    for r_idx, s in enumerate(s_rings):
        row = []
        for k, phi in enumerate(phis):
            deg = math.degrees(phi)
            adeg = abs(deg)

            # Soft curtain sweep away from center part (-2 deg) + gentle downward sag on sides!
            sweep_bias = 0.0
            if adeg < 78.0:
                dir_sign = 1.0 if deg >= -2.0 else -1.0
                dist_part = smoothstep(0.0, 14.0, abs(deg - (-2.0)))
                sweep_bias = dir_sign * 0.082 * dist_part * math.sin(s * math.pi) * (1.0 - smoothstep(46.0, 78.0, adeg))

            base_dir = slerp_dir(bun_dir, hl_dirs[k], s)
            if abs(sweep_bias) > 1e-5:
                tangent = Vector((math.cos(phi), -math.sin(phi), 0.0))
                base_dir = (base_dir + tangent * sweep_bias).normalized()

            # Relaxed gravity sag on side sweeps (adeg 55..150) so hair looks loosely draped rather than pulled tight!
            side_sag = bell(adeg, 108.0, 48.0) * math.sin(s * math.pi) * 0.062
            if side_sag > 1e-5:
                base_dir = (base_dir + Vector((0.0, 0.0, -side_sag))).normalized()

            p_sk, n_sk = skull_raycast(bvh, C, base_dir)
            out_dir = (base_dir * 0.60 + n_sk * 0.40).normalized()

            topness = clamp((base_dir.z + 0.05) / 0.95, 0.0, 1.0)
            frontness = smoothstep(82.0, 0.0, adeg)

            # Airy, soft feminine volume (~21-24mm on crown & curtain, +10mm plush loose sweep over/behind ears!)
            side_sweep_swell = bell(adeg, 96.0, 44.0) * bell(s, 0.68, 0.34) * 0.0068
            loose_side_pouch = bell(adeg, 124.0, 36.0) * bell(s, 0.58, 0.38) * 0.0095
            bun_neck_fullness = bell(s, 0.18, 0.28) * 0.0098
            thick = 0.0142 + 0.0092 * (topness ** 0.85) + side_sweep_swell + loose_side_pouch + bun_neck_fullness

            # Soft center part groove on front-crown + plush curtain bang puffs on either side!
            part_groove = bell(deg, -2.0, 8.5) * smoothstep(0.22, 0.92, s) * (1.0 - smoothstep(40.0, 75.0, adeg))
            curtain_puff = (bell(deg, +24.0, 20.0) + bell(deg, -27.0, 20.0)) * smoothstep(0.30, 0.96, s)
            thick -= 0.0068 * part_groove
            thick += 0.0055 * curtain_puff

            # Soft organic loose wave planes
            loose_waves = math.cos(phi * 6.0 + 0.9 * s) * 0.0028 * math.sin(s * math.pi * 0.90)
            thick += loose_waves

            if r_idx == len(s_rings) - 1:
                edge_keep = 0.38 + 0.32 * frontness * (1.0 - 0.50 * part_groove)
                thick *= edge_keep
                if 94.0 <= adeg <= 109.8:
                    p_sk.z = max(p_sk.z, 1.710)
            elif r_idx == len(s_rings) - 2:
                thick *= 0.92 + 0.08 * frontness

            thick = max(0.0034, thick)
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
        s = s_rings[r_idx]
        return 0.96 - 0.84 * s  # 0.96 -> 0.12

    r0 = ring_verts[0]
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((v_gather_center, r0[k], r0[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (0.5 * (u0 + u1), 0.98)
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

    r_last = ring_verts[-1]
    v_last = v_of_cap(len(s_rings) - 1)
    v_inner_crown = bm.verts.new(bun_skull - bun_n * 0.0025)
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((r_last[k], inner_lip_verts[k], inner_lip_verts[kn], r_last[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (u0, v_last)
        f.loops[1][uv_lay].uv = (u0, 0.09)
        f.loops[2][uv_lay].uv = (u1, 0.09)
        f.loops[3][uv_lay].uv = (u1, v_last)

        f_in = bm.faces.new((inner_lip_verts[kn], inner_lip_verts[k], v_inner_crown))
        for lp in f_in.loops:
            lp[uv_lay].uv = (0.5, 0.03)

    # =========================================================================
    # 2. PLUSH SPIRAL-COILED LOOSE FEMININE BUN (CHIGNON) + SOFT HAIR-WRAP BASE
    # =========================================================================
    bun_x = Vector((1.0, 0.0, 0.0))
    bun_z = bun_axis.normalized()
    bun_y = bun_z.cross(bun_x).normalized()

    N_BUN = 20
    base_origin = bun_skull + bun_z * 0.0085

    # 2A. Soft Loose Hair-Strand Wrap around Bun Base (warm chestnut UVs — NO dark elastic band!)
    wrap_stations = [
        (0.000, 0.035, 0.032, 0.66),
        (0.009, 0.041, 0.037, 0.56),
        (0.018, 0.039, 0.035, 0.48),
    ]
    wrap_rings = []
    for ax_d, rx, ry, _ in wrap_stations:
        c_pt = base_origin + bun_z * ax_d - Vector((0.0, 0.0, 0.0015 * (ax_d / 0.018)))
        ring = []
        for i in range(N_BUN):
            th = (2.0 * math.pi * i) / N_BUN
            wobble = 1.0 + 0.055 * math.cos(3.0 * th + ax_d * 90.0)
            p = c_pt + bun_x * (rx * math.cos(th) * wobble) + bun_y * (ry * math.sin(th) * wobble)
            ring.append(bm.verts.new(p))
        wrap_rings.append(ring)

    v_wrap_root = bm.verts.new(base_origin)
    v_wrap_top = bm.verts.new(base_origin + bun_z * 0.018)
    for i in range(N_BUN):
        inext = (i + 1) % N_BUN
        fb = bm.faces.new((v_wrap_root, wrap_rings[0][inext], wrap_rings[0][i]))
        for lp in fb.loops:
            lp[uv_lay].uv = (0.5, 0.66)
        ft = bm.faces.new((wrap_rings[-1][i], wrap_rings[-1][inext], v_wrap_top))
        for lp in ft.loops:
            lp[uv_lay].uv = (0.5, 0.48)

    for s_i in range(len(wrap_stations) - 1):
        ra, rb = wrap_rings[s_i], wrap_rings[s_i + 1]
        va, vb = wrap_stations[s_i][3], wrap_stations[s_i + 1][3]
        for i in range(N_BUN):
            inext = (i + 1) % N_BUN
            # Diagonal UV twist so loose hair strands visibly wrap around the bun base!
            u0, u1 = (i / N_BUN), ((i + 1) / N_BUN)
            f = bm.faces.new((ra[i], rb[i], rb[inext], ra[inext]))
            f.loops[0][uv_lay].uv = (u0, va)
            f.loops[1][uv_lay].uv = (u0 + 0.08, vb)
            f.loops[2][uv_lay].uv = (u1 + 0.08, vb)
            f.loops[3][uv_lay].uv = (u1, va)

    # 2B. Plush, Airy, Voluminous Loose Chignon (~112mm wide x 104mm tall with soft gravity droop!)
    bun_stations = [
        # (axial_dist, rx_width, ry_height, v_uv)
        (0.013, 0.036, 0.033, 0.84),
        (0.026, 0.052, 0.048, 0.72),
        (0.043, 0.057, 0.053, 0.58),
        (0.060, 0.052, 0.048, 0.44),
        (0.074, 0.039, 0.035, 0.31),
        (0.084, 0.020, 0.018, 0.20),
    ]
    bun_rings = []
    n_bst = len(bun_stations)
    for s_idx, (ax_d, rx, ry, _) in enumerate(bun_stations):
        s_norm = s_idx / (n_bst - 1)
        # Soft gravity sag (-Z) + organic spiral coil shift so the bun reads as loosely coiled locks!
        grav_droop = Vector((0.0, 0.0, -0.0095 * (s_norm ** 1.30)))
        coil_shift = (
            bun_x * (0.0038 * math.cos(s_norm * math.pi * 2.0 + 0.3))
            + bun_y * (0.0032 * math.sin(s_norm * math.pi * 2.0 + 0.3))
            + grav_droop
        )
        c_pt = base_origin + bun_z * ax_d + coil_shift
        mid_f = math.sin(s_norm * math.pi)
        ring = []
        for i in range(N_BUN):
            th = (2.0 * math.pi * i) / N_BUN
            # Multi-coil loose spiral silhouette (2-lobe + 3-lobe + 5-sub-strand relief)
            lobe2 = 1.0 + 0.095 * math.cos(2.0 * th + 2.9 * s_norm) * mid_f
            lobe3 = 1.0 + 0.055 * math.cos(3.0 * th - 2.1 * s_norm + 0.7) * mid_f
            sub_coil = 1.0 + 0.038 * math.cos(5.0 * th + 3.4 * s_norm) * mid_f
            sc = lobe2 * lobe3 * sub_coil
            p = c_pt + bun_x * (rx * math.cos(th) * sc) + bun_y * (ry * math.sin(th) * sc)
            ring.append(bm.verts.new(p))
        bun_rings.append(ring)

    v_bun_root = bm.verts.new(base_origin + bun_z * 0.012)
    # Tucked spiral dimple at center of loose bun!
    v_bun_dimple = bm.verts.new(base_origin + bun_z * 0.0815 + Vector((0.0, 0.0, -0.0095)))

    for i in range(N_BUN):
        inext = (i + 1) % N_BUN
        fr = bm.faces.new((v_bun_root, bun_rings[0][inext], bun_rings[0][i]))
        for lp in fr.loops:
            lp[uv_lay].uv = (0.5, 0.86)

    for s_i in range(n_bst - 1):
        ra, rb = bun_rings[s_i], bun_rings[s_i + 1]
        va, vb = bun_stations[s_i][3], bun_stations[s_i + 1][3]
        # Twist U per station so texture strands visibly spiral into the center of the bun!
        tw0 = 0.065 * s_i
        tw1 = 0.065 * (s_i + 1)
        for i in range(N_BUN):
            inext = (i + 1) % N_BUN
            u0, u1 = (i / N_BUN), ((i + 1) / N_BUN)
            f = bm.faces.new((ra[i], rb[i], rb[inext], ra[inext]))
            f.loops[0][uv_lay].uv = (u0 + tw0, va)
            f.loops[1][uv_lay].uv = (u0 + tw1, vb)
            f.loops[2][uv_lay].uv = (u1 + tw1, vb)
            f.loops[3][uv_lay].uv = (u1 + tw0, va)

    last_ring = bun_rings[-1]
    v_last_b = bun_stations[-1][3]
    tw_last = 0.065 * (n_bst - 1)
    for i in range(N_BUN):
        inext = (i + 1) % N_BUN
        u0, u1 = (i / N_BUN) + tw_last, ((i + 1) / N_BUN) + tw_last
        ft = bm.faces.new((last_ring[i], v_bun_dimple, last_ring[inext]))
        ft.loops[0][uv_lay].uv = (u0, v_last_b)
        ft.loops[1][uv_lay].uv = (0.5 * (u0 + u1), 0.16)
        ft.loops[2][uv_lay].uv = (u1, v_last_b)

    # =========================================================================
    # 3. DRAPED SIDE-SWEEP WAVES + CURTAIN JAW TENDRILS + NECK-HUGGING NAPE WISPS
    # =========================================================================
    def add_soft_tendril(spine_pts, out_nrms, widths, thicknesses, u_c=0.52, v_top=0.78, v_bot=0.12):
        n_seg = len(spine_pts)
        sec = []
        for idx, pt in enumerate(spine_pts):
            no = out_nrms[idx].normalized()
            tang = (spine_pts[idx + 1] - pt).normalized() if idx < n_seg - 1 else (pt - spine_pts[idx - 1]).normalized()
            side = tang.cross(no).normalized()
            wh = widths[idx] * 0.5
            thk = thicknesses[idx]
            back_sink = 0.0014 if idx < n_seg - 1 else 0.0006
            v_l = bm.verts.new(pt - side * wh - no * back_sink)
            v_r = bm.verts.new(pt + side * wh - no * back_sink)
            v_t = bm.verts.new(pt + no * thk)
            sec.append((v_l, v_t, v_r))

        rl, rt, rr = sec[0]
        f0 = bm.faces.new((rl, rr, rt))
        for lp in f0.loops:
            lp[uv_lay].uv = (u_c, v_top)

        tl, tt, tr = sec[-1]
        ftip = bm.faces.new((tl, tt, tr))
        ftip.loops[0][uv_lay].uv = (u_c - 0.04, v_bot)
        ftip.loops[1][uv_lay].uv = (u_c, v_bot + 0.01)
        ftip.loops[2][uv_lay].uv = (u_c + 0.04, v_bot)

        du = 0.068
        for s_i in range(n_seg - 1):
            v0 = v_top - (v_top - v_bot) * (s_i / (n_seg - 1))
            v1 = v_top - (v_top - v_bot) * ((s_i + 1) / (n_seg - 1))
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

    def skull_surf_pt(x, y, z, lift=0.016):
        d = (Vector((x, y, z)) - C).normalized()
        p_sk, n_sk = skull_raycast(bvh, C, d)
        n_out = (d * 0.60 + n_sk * 0.40).normalized()
        return p_sk + n_out * lift, n_out

    # 3A. Seamless Curtain-Bang -> Wavy Face-Framing Jaw Tendrils (emerge softly from curtain arch!)
    for side_sign, u_c, asym in [(+1.0, 0.58, 0.0), (-1.0, 0.38, 0.010)]:
        p0, n0 = skull_surf_pt(side_sign * 0.024, 0.030, 1.754, 0.012)
        p1, n1 = skull_surf_pt(side_sign * 0.056, 0.010, 1.734, 0.014)
        p2, n2 = skull_surf_pt(side_sign * 0.078, -0.024, 1.698, 0.014)
        t_pts = [
            p0,
            p1,
            p2,
            # Soft outward cheek S-wave well in front of ear (y=-0.038m -> 45mm clear of ear!)
            Vector((side_sign * 0.085, -0.038, 1.648)),
            # Gentle feminine inward taper framing the jawline
            Vector((side_sign * 0.080, -0.028, 1.602 + asym * 0.5)),
            Vector((side_sign * 0.070, -0.016, 1.564 + asym)),
        ]
        t_nrms = [
            n0,
            n1,
            n2,
            Vector((side_sign * 0.93, 0.32, 0.05)),
            Vector((side_sign * 0.89, 0.41, -0.10)),
            Vector((side_sign * 0.85, 0.46, -0.16)),
        ]
        add_soft_tendril(
            t_pts,
            t_nrms,
            widths=[0.022, 0.030, 0.031, 0.028, 0.020, 0.009],
            thicknesses=[0.0022, 0.0040, 0.0048, 0.0048, 0.0038, 0.0018],
            u_c=u_c,
            v_top=0.78,
            v_bot=0.14,
        )

    # 3B. Soft Romantic Escaping Nape Curl (drapes naturally along the nape without twin pegs!)
    def nape_surf_pt(side_x: float, z_h: float, lift: float):
        orig = Vector((0.0, -0.082, z_h))
        d = Vector((side_x, -0.94, 0.0)).normalized()
        p_hit, n_hit = skull_raycast(bvh, orig, d, fallback_r=0.052)
        n_out = Vector((d.x * 0.55 + n_hit.x * 0.45, d.y * 0.55 + n_hit.y * 0.45, -0.05)).normalized()
        return Vector((p_hit.x, p_hit.y, z_h)) + n_out * lift, n_out

    w0, nw0 = nape_surf_pt(+0.22, 1.634, 0.0042)
    w1, nw1 = nape_surf_pt(+0.30, 1.608, 0.0078)
    w2, nw2 = nape_surf_pt(+0.18, 1.583, 0.0088)
    w3, nw3 = nape_surf_pt(+0.06, 1.563, 0.0068)
    add_soft_tendril(
        [w0, w1, w2, w3],
        [nw0, nw1, nw2, nw3],
        widths=[0.028, 0.023, 0.015, 0.007],
        thicknesses=[0.0026, 0.0038, 0.0030, 0.0014],
        u_c=0.53,
        v_top=0.76,
        v_bot=0.14,
    )

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])

    mesh = bpy.data.meshes.new("CoqueSoltoFeminino")
    bm.to_mesh(mesh)
    bm.free()

    for poly in mesh.polygons:
        poly.use_smooth = False

    hair_obj = bpy.data.objects.new("CoqueSoltoFeminino", mesh)
    bpy.context.scene.collection.objects.link(hair_obj)
    return hair_obj


def verify_female_bun_fit(hair_obj):
    """Verify triangle budget, ear clearance, brow clearance, and loose bun dimensions."""
    bm = bmesh.new()
    bm.from_mesh(hair_obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    tri_count = len(bm.faces)
    verts = [v.co.copy() for v in bm.verts]
    bm.free()

    ear_zone = [p for p in verts if -0.1028 <= p.y <= -0.0825 and abs(p.x) > 0.078 and p.z > 1.64]
    min_ear_z = min((p.z for p in ear_zone), default=9.9)
    brow_zone = [p for p in verts if p.y > 0.032 and abs(p.x) < 0.040 and p.z > 1.68]
    min_brow_z = min((p.z for p in brow_zone), default=9.9)
    min_bun_y = min(p.y for p in verts)
    min_tendril_z = min((p.z for p in verts if p.y > -0.06), default=9.9)

    print(f"[female-bun-verify] triangles={tri_count}, verts={len(verts)}")
    print(f"[female-bun-verify] ear_arch_min_z={min_ear_z:.4f}m (clearance={(min_ear_z - 1.7065)*1000:.1f}mm)")
    print(f"[female-bun-verify] forehead_min_z={min_brow_z:.4f}m (clearance={(min_brow_z - 1.718)*1000:.1f}mm)")
    print(f"[female-bun-verify] bun_back_y={min_bun_y:.4f}m, jaw_tendril_min_z={min_tendril_z:.4f}m")
    assert min_ear_z >= 1.7085, f"Hair encroaches on ear! min_ear_z={min_ear_z}"
    assert min_brow_z >= 1.730, f"Hair hangs too low over brow! min_brow_z={min_brow_z}"
    return tri_count


def export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path: pathlib.Path):
    """Export CoqueSoltoFeminino.glb with head_socket pivot & embedded 128x128 PNG."""
    head_pivot = arm_obj.data.bones["head"].head_local.copy()

    hair_obj.name = "CoqueSoltoFeminino_src"
    hair_obj.data.name = "CoqueSoltoFeminino_src"

    pivot_empty = bpy.data.objects.new("head_socket", None)
    bpy.context.scene.collection.objects.link(pivot_empty)
    pivot_empty.location = head_pivot

    export_copy = hair_obj.copy()
    export_copy.data = hair_obj.data.copy()
    export_copy.name = "CoqueSoltoFeminino"
    export_copy.data.name = "CoqueSoltoFeminino"
    export_copy.data.transform(Matrix.Translation(-head_pivot))
    export_copy.parent = pivot_empty
    export_copy.matrix_parent_inverse = Matrix.Identity(4)
    export_copy.location = (0.0, 0.0, 0.0)
    bpy.context.scene.collection.objects.link(export_copy)

    mat = bpy.data.materials.new("coque_solto_feminino")
    mat.use_nodes = True
    mat.use_backface_culling = False
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.85
    bsdf.inputs["Metallic"].default_value = 0.0
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.15
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
    hair_obj.name = "CoqueSoltoFeminino"
    hair_obj.data.name = "CoqueSoltoFeminino"

    raw = glb_path.read_bytes()
    magic, version, total_len = struct.unpack_from("<4sII", raw, 0)
    assert magic == b"glTF" and version == 2 and total_len == len(raw)
    jlen = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20 : 20 + jlen])
    print(f"[glb-verify] {glb_path.name} ({len(raw)} bytes), images={len(doc.get('images', []))}, meshes={len(doc.get('meshes', []))}")


def render_female_bun_previews(hair_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render 4-angle close-ups on Female chartest + Full-body Female & isolated 3D piece."""
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

    world = bpy.data.worlds.new("FemaleBunPreviewWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.055, 0.064, 0.080, 1.0)
    bg.inputs[1].default_value = 1.0

    lights = []
    for lname, loc, energy, col in [
        ("FBKey", (-2.0, 3.6, 3.4), 4.4, (1.0, 0.97, 0.92)),
        ("FBFill", (2.8, 2.6, 2.2), 2.2, (0.82, 0.88, 1.0)),
        ("FBRim", (0.2, -3.5, 2.9), 3.0, (0.95, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy = energy
        ldata.color = col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.70)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("FemaleBunCam")
    cam = bpy.data.objects.new("FemaleBunCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    # Dedicated clean Female material using Chartest-Female-v01-128.png
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
    hair_obj.hide_render = True

    def spawn_pair(x_pos, yaw_deg, include_body=True, is_female=True):
        arm_obj["height_m"] = 1.82
        arm_obj["is_female"] = is_female
        arm_obj["breasts_size"] = 1.0 if is_female else 0.0
        arm_obj.update_tag()
        body_obj.update_tag()
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

    # --- SHEET 1: 4-Angle Head Close-up on Female chartest (Front 0°, 3/4 +38°, Pure Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(0.58, 0.0), (0.20, 38.0), (-0.19, -90.0), (-0.59, -142.0)]:
        snaps.extend(spawn_pair(x_p, yaw, include_body=True, is_female=True))

    scene.render.resolution_x = 1660
    scene.render.resolution_y = 720
    cam.location = (-0.01, 3.0, 1.70)
    cam.rotation_euler = (Vector((-0.01, 0.0, 1.68)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 1.62
    scene.render.filepath = str(out_dir / "preview_female_bun_angles.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    # --- SHEET 2: Full-Body Female (Front-3/4 & Back-3/4) + Isolated 3D Loose Bun Piece
    snaps = []
    snaps.extend(spawn_pair(1.55, 28.0, include_body=True, is_female=True))
    snaps.extend(spawn_pair(0.48, -138.0, include_body=True, is_female=True))
    for x_p, yaw in [(-0.62, 38.0), (-1.58, -130.0)]:
        iso = spawn_pair(x_p, yaw, include_body=False)
        for o in iso:
            o.scale = (2.4, 2.4, 2.4)
            o.location = (x_p, 0.0, 1.00 - 1.70 * 2.4)
        snaps.extend(iso)

    scene.render.resolution_x = 1600
    scene.render.resolution_y = 800
    cam.location = (0.0, 5.0, 0.96)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.55
    scene.render.filepath = str(out_dir / "preview_female_bun_full_and_iso.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    arm_obj["is_female"] = True
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

    hair_obj = build_loose_female_bun_mesh(body_obj)
    verify_female_bun_fit(hair_obj)

    tex_png = OUT_DIR / "coque_solto_feminino_128.png"
    uv_png = OUT_DIR / "coque_solto_feminino_uv.png"
    glb_path = OUT_DIR / "CoqueSoltoFeminino.glb"
    blend_out = OUT_DIR / "coque_solto_feminino.blend"

    tex_img = generate_psx_wavy_bun_texture(tex_png)
    export_uv_overlay(hair_obj, uv_png)
    export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path)

    hair_obj.parent = arm_obj
    hair_obj.parent_type = "BONE"
    hair_obj.parent_bone = "head"
    head_bone = arm_obj.pose.bones["head"]
    hair_obj.matrix_parent_inverse = (arm_obj.matrix_world @ head_bone.matrix).inverted()

    arm_obj["is_female"] = True
    bpy.ops.wm.save_as_mainfile(filepath=str(blend_out))
    render_female_bun_previews(hair_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CoqueSoltoFeminino built & rendered successfully!")


if __name__ == "__main__":
    main()
