"""Build Grimora low-poly PSX Long Loose Hair (CabeloLongoSolto) fitted to chartest (Male & Female).

Generates in the conversation staging directory (prior to user approval):
- CabeloLongoSolto.glb                     (embedded 128x128 PSX hair texture, head_socket hierarchy)
- cabelo_longo_solto_128.png               (128x128 dark-fantasy PSX long strand texture)
- cabelo_longo_solto_uv.png                (128x128 transparent UV overlay)
- cabelo_longo_solto.blend                 (authoring scene parented to chartest_rig -> head)
- preview_long_hair_angles.png             (4-angle upper-body turnaround: Front, 3/4, Side, 3/4 Back)
- preview_long_hair_male_female_and_iso.png (Full-body Male & Female + isolated 3D hair piece)
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
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\hair_long_loose"
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
    return ((math.sin((1.0 - t) * omega) / so) * a + (math.sin(t * omega) * omega / omega if False else math.sin(t * omega) / so) * b).normalized()


def generate_psx_long_hair_texture(png_path: pathlib.Path, w: int = 128, h: int = 128):
    """Author a gritty 128x128 PSX dark-fantasy long flowing hair texture matching Grimora's grain.

    UV layout convention:
    - V in [0.09 .. 0.98]: continuous flowing strands from crown root (V=0.98) through mid-cap (V=0.52)
      down the shoulder/back mantle to layered lock tips (V=0.09).
    - U in [0.00 .. 1.00): seamless periodic azimuthal locks around the head/mantle.
    - V in [0.00 .. 0.07]: deep dark interior lining / underside shadow strip.
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

    # Rich dark-fantasy espresso/chestnut brown with dual sheen bands (crown halo + mid-back sheen)
    root_col = (0.060, 0.041, 0.030)
    mid_col = (0.205, 0.136, 0.090)
    hi_col = (0.350, 0.244, 0.164)
    shadow_col = (0.038, 0.026, 0.019)

    for y in range(h):
        v = (y + 0.5) / h
        # Bottom strip V < 0.075: deep warm under-hair shadow (dark brown, NOT pitch-black void,
        # so neck-framing under-layers read as shaded brown hair from the front!)
        if v < 0.075:
            for x in range(w):
                u_sh = (x + 0.5) / w
                lock_sh = 0.78 + 0.44 * periodic_noise_1d(u_sh, 20, 91)
                jit = (0.94 + 0.12 * h2(x, y)) * lock_sh
                idx = (y * w + x) * 4
                px[idx : idx + 4] = [
                    0.088 * jit,
                    0.058 * jit,
                    0.039 * jit,
                    1.0,
                ]
            continue

        vq = (int(y // 2) + 0.5) / (h / 2.0)

        for x in range(w):
            u_raw = (x + 0.5) / w
            # Gentle flowing wave down the long hair length
            wave = math.sin(vq * 9.5 + periodic_noise_1d(u_raw, 12, 5) * 3.0) * 0.010
            wave += (periodic_noise_1d(u_raw, 16, int(y // 12) + 41) - 0.5) * 0.012
            u = (u_raw + wave) % 1.0

            broad_lock = periodic_noise_1d(u, 14, 7)
            med_lock = periodic_noise_1d(u, 30, 19)
            fine_strand = periodic_noise_1d(u, 64, 37 + (y // 20))
            micro_stroke = h2(x, y // 3)

            strand = (
                0.36 * broad_lock
                + 0.32 * med_lock
                + 0.20 * fine_strand
                + 0.12 * micro_stroke
            )

            # Dual natural anisotropic hair sheen: upper crown band (v~0.76) + shoulder/back cascade band (v~0.38)
            off = (periodic_noise_1d(u, 16, 61) - 0.5) * 0.12
            spec1 = bell(vq, 0.75 + off, 0.16)
            spec2 = bell(vq, 0.37 - off * 0.8, 0.18)
            spec_band = (spec1 + 0.80 * spec2) * smoothstep(0.40, 0.78, strand)

            crown_dark = smoothstep(0.88, 0.99, vq)
            neck_dip_ao = bell(vq, 0.55, 0.09) * 0.22  # subtle natural shadow at nape/neck curve
            tip_dark = smoothstep(0.18, 0.08, vq)

            # Center-left natural part crease shadow (u ~ 0.48)
            part_shadow = bell(u_raw, 0.483, 0.024) * smoothstep(0.55, 0.82, vq) * 0.48

            shade = 0.60 + 0.74 * strand
            r = mid_col[0] * shade + (hi_col[0] - mid_col[0]) * spec_band * 0.86
            g = mid_col[1] * shade + (hi_col[1] - mid_col[1]) * spec_band * 0.86
            b = mid_col[2] * shade + (hi_col[2] - mid_col[2]) * spec_band * 0.86

            dark_f = clamp(0.72 * crown_dark + neck_dip_ao + 0.60 * tip_dark + part_shadow, 0.0, 0.90)
            r = r * (1.0 - dark_f) + root_col[0] * dark_f
            g = g * (1.0 - dark_f) + root_col[1] * dark_f
            b = b * (1.0 - dark_f) + root_col[2] * dark_f

            checker = 1.045 if ((x + y) & 1) == 0 else 0.955
            jitter = 0.94 + 0.12 * h2(x, y)
            r = clamp(r * checker * jitter, shadow_col[0], 0.47)
            g = clamp(g * checker * jitter, shadow_col[1], 0.35)
            b = clamp(b * checker * jitter, shadow_col[2], 0.25)

            idx = (y * w + x) * 4
            px[idx : idx + 4] = [r, g, b, 1.0]

    img = bpy.data.images.new("cabelo_longo_solto_128", width=w, height=h, alpha=True)
    img.pixels[:] = px
    img.filepath_raw = str(png_path)
    img.file_format = "PNG"
    img.save()
    return img


def export_uv_overlay(mesh_obj, uv_png_path: pathlib.Path, size: int = 128):
    """Write a 128x128 transparent PNG with crisp 1px UV edges for optional hand-painting."""
    buf = [0.0] * (size * size * 4)

    def put_px(ix, iy, rgba):
        if 0 <= ix < size and 0 <= iy < size:
            o = (iy * size + ix) * 4
            buf[o : o + 4] = rgba

    def draw_line(u0, v0, u1, v1):
        # Wrap into [0, 1] cleanly
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


def build_combined_upper_body_bvh(body_obj, arm_obj) -> BVHTree:
    """Combined outer-envelope BVH of BOTH Male and Female chartest upper bodies (z >= 1.05m)."""
    dg = bpy.context.evaluated_depsgraph_get()
    bm_all = bmesh.new()

    for fem, bsize in [(False, 0.0), (True, 1.0)]:
        arm_obj["is_female"] = fem
        arm_obj["breasts_size"] = bsize
        arm_obj["height_m"] = 1.82
        arm_obj.update_tag()
        body_obj.update_tag()
        dg.update()

        ev = body_obj.evaluated_get(dg)
        tmp_mesh = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=False, depsgraph=dg)
        bm_part = bmesh.new()
        bm_part.from_mesh(tmp_mesh)
        bpy.data.meshes.remove(tmp_mesh)

        del_v = [v for v in bm_part.verts if v.co.z < 1.05]
        bmesh.ops.delete(bm_part, geom=del_v, context="VERTS")
        bmesh.ops.triangulate(bm_part, faces=bm_part.faces[:])

        # Copy into bm_all
        vmap = {v.index: bm_all.verts.new(v.co) for v in bm_part.verts}
        for f in bm_part.faces:
            bm_all.faces.new([vmap[v.index] for v in f.verts])
        bm_part.free()

    # Restore default male state
    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 1.0
    arm_obj.update_tag()
    body_obj.update_tag()
    dg.update()

    bvh = BVHTree.FromBMesh(bm_all)
    bm_all.free()
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


def cap_perimeter_z(phi: float) -> float:
    """Perimeter Z(phi) of the cranial cap before the back columns extend into the long mantle."""
    deg = math.degrees(phi)
    adeg = abs(deg)

    # 1. Center-parted natural curtain forehead: higher inverted-V opening at center part (-3 deg),
    #    sweeping smoothly down along both outer brows/temples so it reads as parted loose locks!
    z = 1.738
    z += 0.019 * bell(deg, -3.0, 18.0)
    z -= 0.008 * bell(deg, +30.0, 18.0)
    z -= 0.008 * bell(deg, -33.0, 18.0)

    # 2. Graceful temple sweep descending in front of ear (adeg 45..86 -> dips to z ~ 1.668m)
    temple_drop = smoothstep(45.0, 76.0, adeg)
    z -= 0.030 * temple_drop
    cheek_lock = bell(adeg, 79.5, 10.0)
    z -= 0.036 * cheek_lock

    # 3. Snug Ear Cutout Arch (adeg 94..109.6 -> z ~ 1.710..1.713m right over ear helix!)
    ear_arch = smoothstep(87.0, 94.5, adeg) * (1.0 - smoothstep(108.5, 113.0, adeg))
    z = z * (1.0 - ear_arch) + (1.710 + 0.0035 * bell(adeg, 101.5, 8.5)) * ear_arch

    # 4. Back cranial cap transition line (adeg >= 110 -> occipital base z ~ 1.666m)
    occiput_t = smoothstep(109.5, 124.0, adeg)
    z = z * (1.0 - occiput_t) + 1.666 * occiput_t

    return z


def build_long_loose_hair_mesh(body_obj, arm_obj) -> bpy.types.Object:
    """Sculpt solid 3D PSX Long Loose Hair with shoulder/back mantle & collarbone locks."""
    head_bvh = build_head_bvh(body_obj)
    body_bvh = build_combined_upper_body_bvh(body_obj, arm_obj)

    C = Vector((0.0, -0.075, 1.690))

    # Crown part/whorl origin slightly left of center
    whorl_dir = Vector((-0.008, -0.018, 0.130)).normalized()
    whorl_skull, whorl_n = skull_raycast(head_bvh, C, whorl_dir)
    whorl_outer = whorl_skull + whorl_n * 0.0185

    N_AZ = 40
    s_rings = [0.16, 0.34, 0.54, 0.76, 0.91, 1.00]

    bm = bmesh.new()
    uv_lay = bm.loops.layers.uv.new("UVMap")

    phis = []
    for k in range(N_AZ):
        raw_phi = -math.pi + (2.0 * math.pi * k) / N_AZ
        raw_deg = math.degrees(raw_phi)
        sign = 1.0 if raw_deg >= 0.0 else -1.0
        # Align +-108 deg column to +-109.6 deg right behind the upper-back ear tip
        warped_deg = raw_deg + sign * 1.6 * bell(abs(raw_deg), 108.0, 8.0)
        phis.append(math.radians(warped_deg))

    hl_dirs = []
    for phi in phis:
        zh = cap_perimeter_z(phi)
        axis_pt = Vector((0.0, -0.075, clamp(zh, 1.640, 1.775)))
        horiz_dir = Vector((math.sin(phi), math.cos(phi), 0.0)).normalized()
        p_hit, _ = skull_raycast(head_bvh, axis_pt, horiz_dir, fallback_r=0.082)
        p_target = Vector((p_hit.x, p_hit.y, zh))
        hl_dirs.append((p_target - C).normalized())

    v_center = bm.verts.new(whorl_outer)
    ring_verts = []
    cap_inner_lip = []  # inner vertices along s=1.0

    for r_idx, s in enumerate(s_rings):
        row = []
        for k, phi in enumerate(phis):
            deg = math.degrees(phi)
            adeg = abs(deg)

            # Curtain part sweep away from deg = -6 deg toward both temples!
            sweep_bias = 0.0
            if adeg < 70.0:
                dir_sign = 1.0 if deg >= -6.0 else -1.0
                dist_part = smoothstep(0.0, 16.0, abs(deg - (-6.0)))
                sweep_bias = dir_sign * 0.075 * dist_part * math.sin(s * math.pi) * (1.0 - smoothstep(44.0, 70.0, adeg))

            base_dir = slerp_dir(whorl_dir, hl_dirs[k], s)
            if abs(sweep_bias) > 1e-5:
                tangent = Vector((math.cos(phi), -math.sin(phi), 0.0))
                base_dir = (base_dir + tangent * sweep_bias).normalized()

            p_sk, n_sk = skull_raycast(head_bvh, C, base_dir)
            out_dir = (base_dir * 0.62 + n_sk * 0.38).normalized()

            topness = clamp((base_dir.z + 0.05) / 0.95, 0.0, 1.0)
            frontness = smoothstep(85.0, 0.0, adeg)
            backness = smoothstep(108.0, 145.0, adeg)

            # Lush full volume for long loose hair (~20-23mm on crown/sides/back so it flows naturally!)
            thick = 0.0135 + 0.0095 * (topness ** 0.85) + 0.0035 * frontness * smoothstep(0.25, 0.85, s)
            thick += 0.0035 * backness * smoothstep(0.45, 1.0, s)

            # Crisp 3D center-left part groove along deg = -5.0 + lush curtain crests on both sides!
            part_groove = bell(deg, -5.0, 8.5) * smoothstep(0.16, 0.95, s)
            curtain_crest_r = bell(deg, +20.0, 20.0) * smoothstep(0.30, 0.95, s)
            curtain_crest_l = bell(deg, -28.0, 19.0) * smoothstep(0.30, 0.95, s)
            thick -= 0.0065 * part_groove
            thick += 0.0045 * (curtain_crest_r + curtain_crest_l)

            # Subtle PSX lock clumping planes around the cap
            thick += math.cos(phi * 8.0) * 0.0018 * math.sin(s * math.pi * 0.90)

            if r_idx == len(s_rings) - 1:
                if adeg < 109.0:
                    # Front forehead curtain & ear arch taper cleanly toward skin
                    edge_keep = 0.34 + 0.34 * frontness * (1.0 - 0.50 * part_groove)
                    thick *= edge_keep
                else:
                    # Back/post-ear columns keep full lush thickness (~16mm) to feed seamlessly into the mantle!
                    thick *= 0.62 + 0.38 * smoothstep(109.6, 122.0, adeg)

                if 94.0 <= adeg <= 109.8:
                    p_sk.z = max(p_sk.z, 1.710)

            thick = max(0.0034, thick)
            p_hair = p_sk + out_dir * thick
            row.append(bm.verts.new(p_hair))

            if r_idx == len(s_rings) - 1:
                if adeg < 109.0:
                    lip_pt = p_sk - out_dir * 0.0018 + Vector((0.0, 0.0, 0.0015))
                    if 94.0 <= adeg <= 109.8:
                        lip_pt.z = max(lip_pt.z, 1.7095)
                else:
                    # For mantle columns, inner surface starts 5mm outside skull so mantle has real thickness!
                    lip_pt = p_sk + out_dir * 0.0045
                cap_inner_lip.append(bm.verts.new(lip_pt))

        ring_verts.append(row)

    # --- UV Mapping Helpers (Crown = 0.98, Cap Rim = 0.56, Mantle Hem = 0.10) ---
    def u_of(k_idx):
        return k_idx / N_AZ

    def v_of_cap_ring(r_idx):
        s = s_rings[r_idx]
        return 0.98 - 0.42 * s  # 0.98 -> 0.56 at cap base!

    # 1. Crown fan
    r0 = ring_verts[0]
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((v_center, r0[k], r0[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (0.5 * (u0 + u1), 0.99)
        f.loops[1][uv_lay].uv = (u0, v_of_cap_ring(0))
        f.loops[2][uv_lay].uv = (u1, v_of_cap_ring(0))

    # 2. Cap quad strips
    for r_idx in range(len(s_rings) - 1):
        ra = ring_verts[r_idx]
        rb = ring_verts[r_idx + 1]
        va, vb = v_of_cap_ring(r_idx), v_of_cap_ring(r_idx + 1)
        for k in range(N_AZ):
            kn = (k + 1) % N_AZ
            f = bm.faces.new((ra[k], rb[k], rb[kn], ra[kn]))
            u0, u1 = u_of(k), u_of(k + 1)
            f.loops[0][uv_lay].uv = (u0, va)
            f.loops[1][uv_lay].uv = (u0, vb)
            f.loops[2][uv_lay].uv = (u1, vb)
            f.loops[3][uv_lay].uv = (u1, va)

    # 3. Seal front & ear perimeter (k = 8 .. 31, i.e., from -109.6 deg across front to +109.6 deg)
    r_last = ring_verts[-1]
    v_cap_end = v_of_cap_ring(len(s_rings) - 1)
    for k in range(8, 32):
        kn = k + 1
        f = bm.faces.new((r_last[k], cap_inner_lip[k], cap_inner_lip[kn], r_last[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (u0, v_cap_end)
        f.loops[1][uv_lay].uv = (u0, 0.04)
        f.loops[2][uv_lay].uv = (u1, 0.04)
        f.loops[3][uv_lay].uv = (u1, v_cap_end)

    # Seal inner cranial dome with dark lining (V=0.03)
    v_inner_crown = bm.verts.new(whorl_skull - whorl_n * 0.0025)
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f_in = bm.faces.new((cap_inner_lip[kn], cap_inner_lip[k], v_inner_crown))
        for lp in f_in.loops:
            lp[uv_lay].uv = (0.5, 0.03)

    # =========================================================================
    # 4. CASCADING BACK & SHOULDER MANTLE (Right post-ear k=32 -> Back -> Left post-ear k=8)
    # =========================================================================
    mantle_cols = [32, 33, 34, 35, 36, 37, 38, 39, 0, 1, 2, 3, 4, 5, 6, 7, 8]
    M = len(mantle_cols)  # 17 columns across 140.8 degrees of back & shoulders!
    mantle_us = [(32 + idx) / N_AZ for idx in range(M)]  # Monotonic 0.80 .. 1.20 (tiles seamlessly!)

    N_DRAPE = 7  # 7 vertical drape segments from occipital base/ears down to mid-back!

    outer_mantle = [[r_last[k] for k in mantle_cols]]
    inner_mantle = [[cap_inner_lip[k] for k in mantle_cols]]

    # Precompute starting coordinates for each mantle column at d=0
    start_outer = [r_last[k].co.copy() for k in mantle_cols]
    start_inner = [cap_inner_lip[k].co.copy() for k in mantle_cols]

    # Compute raw 3D coordinates of inner & outer mantle across d=1..N_DRAPE with BVH collision clearance
    raw_inner_grid = [start_inner]
    raw_outer_grid = [start_outer]

    for d_idx in range(1, N_DRAPE + 1):
        t = d_idx / N_DRAPE  # 0 -> 1 down the hair cascade
        in_row_pts = []
        out_row_pts = []

        for m_i in range(M):
            # Normalized lateral coordinate u_lat in [+1.0 (right shoulder) .. 0.0 (center back) .. -1.0 (left shoulder)]
            u_lat = 1.0 - 2.0 * (m_i / (M - 1))
            au = abs(u_lat)
            sign_x = 1.0 if u_lat >= 0.0 else -1.0

            p0_out = start_outer[m_i]
            p0_in = start_inner[m_i]

            # Natural U-shaped layered hem height z_tip(u_lat):
            # Center-back reaches z=1.155m; soft organic lock waves rise gently toward inner shoulders (z=1.265m)
            lock_scallop = -0.011 * math.cos(u_lat * math.pi * 3.0) * (1.0 - 0.30 * au)
            z_tip = 1.162 + 0.105 * (au ** 1.55) + lock_scallop
            z = p0_out.z * (1.0 - t) + z_tip * t

            # Lateral X spread: stays strictly over the neck, trapezius & scapulae (|x| <= 0.124m),
            # completely medial to the shoulder/arm joints so zero cape-flare ever occurs!
            shoulder_fan = smoothstep(0.04, 0.48, t) * (1.0 - 0.18 * smoothstep(0.70, 1.0, t))
            x_target = sign_x * (abs(p0_out.x) + (0.035 * (au ** 0.85)) * shoulder_fan)

            # Base natural hang Y before sagittal back/trapezius collision check:
            behind_ear_tuck = smoothstep(0.65, 1.0, au) * bell(z, 1.668, 0.055) * 0.010
            neck_tuck = bell(z, 1.575, 0.070) * 0.006 * (1.0 - 0.4 * au)
            back_swell = bell(z, 1.370, 0.165) * 0.038 * (1.0 - 0.25 * au)
            y_base = p0_in.y - behind_ear_tuck + neck_tuck - back_swell

            # Sagittal-only BVH collision check from behind the back straight forward (+Y, slight -Z):
            # Never casts sideways through the A-pose arms!
            ray_dir = Vector((0.0, 1.0, -0.14 * smoothstep(1.42, 1.55, z))).normalized()
            ray_orig = Vector((x_target, -0.38, z + 0.035 * smoothstep(1.42, 1.55, z)))
            hit_loc, _, _, _ = body_bvh.ray_cast(ray_orig, ray_dir, 0.36)

            p_in = Vector((x_target, y_base, z))
            if hit_loc is not None and hit_loc.y < -0.05:
                min_clear = 0.0065
                y_req = hit_loc.y - min_clear
                if p_in.y > y_req:
                    p_in.y = y_req
                if z > 1.44 and hit_loc.z + 0.004 > p_in.z:
                    p_in.z = min(hit_loc.z + 0.004, z + 0.018)

            # Gravity hang rule: below z=1.37m, hang smoothly downward from shoulder-blade peak!
            prev_in = raw_inner_grid[d_idx - 1][m_i]
            if z < 1.38:
                p_in.y = min(p_in.y, prev_in.y + 0.0040)

            if p_in.z > 1.630 and au > 0.65:
                p_in.y = min(p_in.y, -0.1065)

            in_row_pts.append(p_in)

        # Gentle lateral smoothing of Y across interior columns so the back drape is silky-smooth
        smoothed_in = [in_row_pts[0].copy()]
        for m_i in range(1, M - 1):
            p_sm = in_row_pts[m_i].copy()
            p_sm.y = 0.20 * in_row_pts[m_i - 1].y + 0.60 * in_row_pts[m_i].y + 0.20 * in_row_pts[m_i + 1].y
            smoothed_in.append(p_sm)
        smoothed_in.append(in_row_pts[-1].copy())

        for m_i in range(M):
            u_lat = 1.0 - 2.0 * (m_i / (M - 1))
            au = abs(u_lat)
            sign_x = 1.0 if u_lat >= 0.0 else -1.0
            p_in = smoothed_in[m_i]

            edge_fade = math.sin(clamp(m_i / (M - 1)) * math.pi) ** 0.50
            tip_fade = 1.0 - 0.76 * smoothstep(0.76, 1.0, t)
            lock_ridge = 0.0046 * (0.5 + 0.5 * math.cos(u_lat * math.pi * 4.0)) * math.sin(t * math.pi * 0.88)
            thick_m = (0.0105 * edge_fade + 0.0032 + lock_ridge) * tip_fade
            thick_m = max(0.0025, thick_m)

            out_n = Vector((sign_x * (0.36 * au), -1.0 + 0.18 * au, 0.08 * bell(p_in.z, 1.47, 0.08))).normalized()
            p_out = p_in + out_n * thick_m
            if max(p_in.z, p_out.z) > 1.630 and au > 0.65:
                p_out.y = min(p_out.y, -0.1095)
            out_row_pts.append(p_out)

        raw_inner_grid.append(smoothed_in)
        raw_outer_grid.append(out_row_pts)

    # Create BMesh vertices for d = 1..N_DRAPE
    for d_idx in range(1, N_DRAPE + 1):
        outer_mantle.append([bm.verts.new(p) for p in raw_outer_grid[d_idx]])
        inner_mantle.append([bm.verts.new(p) for p in raw_inner_grid[d_idx]])

    # Build Outer Mantle Quads (V flows continuously from 0.56 down to 0.10!) + Dark Inner Mantle Quads
    for d_idx in range(N_DRAPE):
        va = v_cap_end - (v_cap_end - 0.10) * (d_idx / N_DRAPE)
        vb = v_cap_end - (v_cap_end - 0.10) * ((d_idx + 1) / N_DRAPE)
        oa, ob = outer_mantle[d_idx], outer_mantle[d_idx + 1]
        ia, ib = inner_mantle[d_idx], inner_mantle[d_idx + 1]

        for m_i in range(M - 1):
            u0, u1 = mantle_us[m_i], mantle_us[m_i + 1]
            # Outer hair surface (outward normal)
            fo = bm.faces.new((oa[m_i], ob[m_i], ob[m_i + 1], oa[m_i + 1]))
            fo.loops[0][uv_lay].uv = (u0, va)
            fo.loops[1][uv_lay].uv = (u0, vb)
            fo.loops[2][uv_lay].uv = (u1, vb)
            fo.loops[3][uv_lay].uv = (u1, va)

            # Inner under-hair layer facing neck/front (shaded warm brown strand UVs in V=0.015..0.065!)
            v_ia = 0.065 - 0.045 * (d_idx / N_DRAPE)
            v_ib = 0.065 - 0.045 * ((d_idx + 1) / N_DRAPE)
            fi = bm.faces.new((ia[m_i + 1], ib[m_i + 1], ib[m_i], ia[m_i]))
            fi.loops[0][uv_lay].uv = (u1, v_ia)
            fi.loops[1][uv_lay].uv = (u1, v_ib)
            fi.loops[2][uv_lay].uv = (u0, v_ib)
            fi.loops[3][uv_lay].uv = (u0, v_ia)

        # Seal Right border (m_i = 0) & Left border (m_i = M - 1) thickness strips!
        fr = bm.faces.new((oa[0], ia[0], ib[0], ob[0]))
        fr.loops[0][uv_lay].uv = (mantle_us[0], va)
        fr.loops[1][uv_lay].uv = (mantle_us[0], 0.05)
        fr.loops[2][uv_lay].uv = (mantle_us[0], 0.05)
        fr.loops[3][uv_lay].uv = (mantle_us[0], vb)

        fl = bm.faces.new((ob[M - 1], ib[M - 1], ia[M - 1], oa[M - 1]))
        fl.loops[0][uv_lay].uv = (mantle_us[M - 1], vb)
        fl.loops[1][uv_lay].uv = (mantle_us[M - 1], 0.05)
        fl.loops[2][uv_lay].uv = (mantle_us[M - 1], 0.05)
        fl.loops[3][uv_lay].uv = (mantle_us[M - 1], va)

    # Seal Bottom Layered Hem (d = N_DRAPE)
    o_bot, i_bot = outer_mantle[-1], inner_mantle[-1]
    for m_i in range(M - 1):
        u0, u1 = mantle_us[m_i], mantle_us[m_i + 1]
        fb = bm.faces.new((o_bot[m_i], i_bot[m_i], i_bot[m_i + 1], o_bot[m_i + 1]))
        fb.loops[0][uv_lay].uv = (u0, 0.10)
        fb.loops[1][uv_lay].uv = (u0, 0.04)
        fb.loops[2][uv_lay].uv = (u1, 0.04)
        fb.loops[3][uv_lay].uv = (u1, 0.10)

    # =========================================================================
    # 5. SCULPTED FRONT COLLARBONE LOCKS + FACE-FRAMING CURTAIN WAVES
    # =========================================================================
    def add_draped_lock(spine_pts, out_normals, widths, thicknesses, u_center=0.52, v_top=0.82, v_bot=0.11):
        """Build a closed, chunky 3D PSX hair lock ribbon with crisp central ridge & dark underside."""
        n_seg = len(spine_pts)
        sec_verts = []
        for idx in range(n_seg):
            pt = spine_pts[idx]
            n_out = out_normals[idx].normalized()
            if idx < n_seg - 1:
                tang = (spine_pts[idx + 1] - pt).normalized()
            else:
                tang = (pt - spine_pts[idx - 1]).normalized()
            side = tang.cross(n_out).normalized()
            w_half = widths[idx] * 0.5
            h_ridge = thicknesses[idx]
            back_sink = 0.0018 if idx < n_seg - 1 else 0.0008
            v_l = bm.verts.new(pt - side * w_half - n_out * back_sink)
            v_r = bm.verts.new(pt + side * w_half - n_out * back_sink)
            v_top_v = bm.verts.new(pt + n_out * h_ridge)
            sec_verts.append((v_l, v_top_v, v_r))

        r_l, r_t, r_r = sec_verts[0]
        f_root = bm.faces.new((r_l, r_r, r_t))
        for lp in f_root.loops:
            lp[uv_lay].uv = (u_center, v_top)

        t_l, t_t, t_r = sec_verts[-1]
        f_tip = bm.faces.new((t_l, t_t, t_r))
        f_tip.loops[0][uv_lay].uv = (u_center - 0.04, v_bot)
        f_tip.loops[1][uv_lay].uv = (u_center, v_bot + 0.01)
        f_tip.loops[2][uv_lay].uv = (u_center + 0.04, v_bot)

        du = 0.075
        for s_i in range(n_seg - 1):
            v0 = v_top - (v_top - v_bot) * (s_i / (n_seg - 1))
            v1 = v_top - (v_top - v_bot) * ((s_i + 1) / (n_seg - 1))
            cur = sec_verts[s_i]
            nxt = sec_verts[s_i + 1]

            f1 = bm.faces.new((cur[0], nxt[0], nxt[1], cur[1]))
            f1.loops[0][uv_lay].uv = (u_center - du, v0)
            f1.loops[1][uv_lay].uv = (u_center - du, v1)
            f1.loops[2][uv_lay].uv = (u_center, v1)
            f1.loops[3][uv_lay].uv = (u_center, v0)

            f2 = bm.faces.new((cur[1], nxt[1], nxt[2], cur[2]))
            f2.loops[0][uv_lay].uv = (u_center, v0)
            f2.loops[1][uv_lay].uv = (u_center, v1)
            f2.loops[2][uv_lay].uv = (u_center + du, v1)
            f2.loops[3][uv_lay].uv = (u_center + du, v0)

            f3 = bm.faces.new((cur[2], nxt[2], nxt[0], cur[0]))
            for lp in f3.loops:
                lp[uv_lay].uv = (0.5, 0.03)

    def front_chest_drape_pt(x: float, z: float, fallback_y: float, clear: float = 0.0055) -> Vector:
        """Cast straight onto the clavicle / upper pectoral surface so front locks rest with real gravity!"""
        ray_o = Vector((x, 0.25, z + 0.020))
        ray_d = Vector((0.0, -1.0, -0.14)).normalized()
        hit_loc, _, _, _ = body_bvh.ray_cast(ray_o, ray_d, 0.36)
        if hit_loc is not None and hit_loc.y > -0.06:
            return Vector((x, hit_loc.y + clear, z))
        return Vector((x, fallback_y, z))

    def skull_surf_pt(x, y, z, lift=0.016):
        d = (Vector((x, y, z)) - C).normalized()
        p_sk, n_sk = skull_raycast(head_bvh, C, d)
        n_out = (d * 0.62 + n_sk * 0.38).normalized()
        return p_sk + n_out * lift, n_out

    # Broad, Lush Front-Shoulder Loose Waves (Left & Right): emerge smoothly from the temple hair mass,
    # route over the outer collarbone groove (|x|~0.114..0.118m, lateral to the breast mound!), and hang
    # vertically downward with real gravity instead of sticking out forward!
    for side_sign, u_c, asym_z in [(+1.0, 0.62, 0.0), (-1.0, 0.34, 0.010)]:
        p0, n0 = skull_surf_pt(side_sign * 0.056, -0.012, 1.762, 0.014)
        p1, n1 = skull_surf_pt(side_sign * 0.078, -0.036, 1.708, 0.014)
        pt_clav = front_chest_drape_pt(side_sign * 0.110, 1.504, -0.014, clear=0.0055)
        pt_upper = front_chest_drape_pt(side_sign * 0.116, 1.454 + asym_z * 0.4, +0.018, clear=0.0055)
        pt_tip_raw = front_chest_drape_pt(side_sign * 0.114, 1.404 + asym_z, +0.024, clear=0.0050)
        # Ensure natural vertical gravity hang at tip (no forward ski-ramp!)
        pt_tip = Vector((pt_tip_raw.x, max(pt_tip_raw.y, pt_upper.y - 0.004), pt_tip_raw.z))
        pt_tip.y = min(pt_tip.y, pt_upper.y + 0.008)

        s_pts = [
            p0,
            p1,
            Vector((side_sign * 0.085, -0.048, 1.646)),
            Vector((side_sign * 0.094, -0.036, 1.576)),
            pt_clav,
            pt_upper,
            pt_tip,
        ]
        s_nrms = [
            n0,
            n1,
            Vector((side_sign * 0.94, 0.25, 0.06)),
            Vector((side_sign * 0.84, 0.44, 0.22)),
            Vector((side_sign * 0.44, 0.78, 0.44)),
            Vector((side_sign * 0.28, 0.94, 0.16)),
            Vector((side_sign * 0.24, 0.97, 0.02)),
        ]
        add_draped_lock(
            s_pts,
            s_nrms,
            widths=[0.038, 0.044, 0.044, 0.042, 0.042, 0.034, 0.018],
            thicknesses=[0.0030, 0.0050, 0.0065, 0.0070, 0.0068, 0.0055, 0.0025],
            u_center=u_c,
            v_top=0.74,
            v_bot=0.14,
        )

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])

    mesh = bpy.data.meshes.new("CabeloLongoSolto")
    bm.to_mesh(mesh)
    bm.free()

    for poly in mesh.polygons:
        poly.use_smooth = False

    hair_obj = bpy.data.objects.new("CabeloLongoSolto", mesh)
    bpy.context.scene.collection.objects.link(hair_obj)
    return hair_obj


def verify_long_hair_fit(hair_obj):
    """Verify triangle budget, ear clearance, brow clearance, and mid-back drape reach."""
    bm = bmesh.new()
    bm.from_mesh(hair_obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    tri_count = len(bm.faces)
    verts = [v.co.copy() for v in bm.verts]
    bm.free()

    ear_zone = [p for p in verts if -0.1025 <= p.y <= -0.0830 and abs(p.x) > 0.078 and p.z > 1.64]
    min_ear_z = min((p.z for p in ear_zone), default=9.9)
    brow_zone = [p for p in verts if p.y > 0.032 and abs(p.x) < 0.040 and p.z > 1.68]
    min_brow_z = min((p.z for p in brow_zone), default=9.9)
    min_back_z = min(p.z for p in verts if p.y < -0.12)
    front_lock_min_z = min((p.z for p in verts if p.y > 0.02 and p.z < 1.55), default=9.9)

    print(f"[long-hair-verify] triangles={tri_count}, verts={len(verts)}")
    print(f"[long-hair-verify] ear_arch_min_z={min_ear_z:.4f}m (ear top=1.7065m -> clearance={(min_ear_z - 1.7065)*1000:.1f}mm)")
    print(f"[long-hair-verify] forehead_min_z={min_brow_z:.4f}m (brow=1.7180m -> clearance={(min_brow_z - 1.718)*1000:.1f}mm)")
    print(f"[long-hair-verify] back_mantle_min_z={min_back_z:.4f}m, front_collar_lock_min_z={front_lock_min_z:.4f}m")
    assert min_ear_z >= 1.7085, f"Hair encroaches on ear! min_ear_z={min_ear_z}"
    assert min_brow_z >= 1.728, f"Hair hangs too low over brow! min_brow_z={min_brow_z}"
    assert min_back_z <= 1.20, f"Back hair not long enough! min_back_z={min_back_z}"
    return tri_count


def export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path: pathlib.Path):
    """Export CabeloLongoSolto.glb with head_socket pivot & embedded 128x128 PNG."""
    head_pivot = arm_obj.data.bones["head"].head_local.copy()

    pivot_empty = bpy.data.objects.new("head_socket", None)
    bpy.context.scene.collection.objects.link(pivot_empty)
    pivot_empty.location = head_pivot

    export_copy = hair_obj.copy()
    export_copy.data = hair_obj.data.copy()
    export_copy.name = "CabeloLongoSolto"
    export_copy.data.transform(Matrix.Translation(-head_pivot))
    export_copy.parent = pivot_empty
    export_copy.matrix_parent_inverse = Matrix.Identity(4)
    export_copy.location = (0.0, 0.0, 0.0)
    bpy.context.scene.collection.objects.link(export_copy)

    mat = bpy.data.materials.new("cabelo_longo_solto")
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


def render_long_hair_previews(hair_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render 4-angle upper-body close-ups + Male/Female full-body & isolated 3D piece."""
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

    world = bpy.data.worlds.new("LongHairPreviewWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.055, 0.064, 0.080, 1.0)
    bg.inputs[1].default_value = 1.0

    lights = []
    for lname, loc, energy, col in [
        ("LHKey", (-2.2, 3.6, 3.4), 4.4, (1.0, 0.97, 0.92)),
        ("LHFill", (2.8, 2.6, 2.2), 2.2, (0.82, 0.88, 1.0)),
        ("LHRim", (0.2, -3.6, 2.8), 3.0, (0.95, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy = energy
        ldata.color = col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.50)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("LongHairCam")
    cam = bpy.data.objects.new("LongHairCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    depsgraph = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = True
    arm_obj.hide_render = True
    hair_obj.hide_render = True

    def spawn_pair(x_pos, yaw_deg, include_body=True, is_female=False):
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
            # Material 0 already has an active shader driver on arm_obj["is_female"], so snap a dedicated
            # female material copy using Chartest-Female-v01-128.png when is_female=True!
            if is_female:
                fem_img = bpy.data.images.get("Chartest-Female-v01-128.png")
                if fem_img is not None:
                    fmat = bpy.data.materials.new("TempFemSkin")
                    fmat.use_nodes = True
                    ns = fmat.node_tree.nodes
                    ls = fmat.node_tree.links
                    ns.clear()
                    mo = ns.new("ShaderNodeOutputMaterial")
                    bs = ns.new("ShaderNodeBsdfPrincipled")
                    bs.inputs["Roughness"].default_value = 0.9
                    ti = ns.new("ShaderNodeTexImage")
                    ti.image = fem_img
                    ti.interpolation = "Closest"
                    ls.new(ti.outputs["Color"], bs.inputs["Base Color"])
                    ls.new(bs.outputs["BSDF"], mo.inputs["Surface"])
                    ob.data.materials.clear()
                    ob.data.materials.append(fmat)
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

    # --- SHEET 1: 4-Angle Head+Shoulders+Back Close-up (Front 0°, 3/4 +36°, Pure Side -90°, 3/4 Back -145°)
    snaps = []
    for x_p, yaw, fem in [(0.75, 0.0, False), (0.25, 36.0, True), (-0.25, -90.0, False), (-0.75, -145.0, False)]:
        snaps.extend(spawn_pair(x_p, yaw, include_body=True, is_female=fem))

    scene.render.resolution_x = 1680
    scene.render.resolution_y = 820
    cam.location = (0.0, 3.2, 1.51)
    cam.rotation_euler = (Vector((0.0, 0.0, 1.49)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 2.08
    scene.render.filepath = str(out_dir / "preview_long_hair_angles.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    # --- SHEET 2: Full-Body Male + Female + Isolated 3D Long Hair Views
    snaps = []
    snaps.extend(spawn_pair(1.85, -24.0, include_body=True, is_female=False))
    snaps.extend(spawn_pair(0.75, 26.0, include_body=True, is_female=True))
    snaps.extend(spawn_pair(-0.35, -148.0, include_body=True, is_female=True))
    for x_p, yaw in [(-1.32, 32.0), (-2.05, -142.0)]:
        iso = spawn_pair(x_p, yaw, include_body=False)
        for o in iso:
            o.scale = (2.0, 2.0, 2.0)
            o.location = (x_p, 0.0, 0.98 - 1.50 * 2.0)
        snaps.extend(iso)

    scene.render.resolution_x = 1720
    scene.render.resolution_y = 820
    cam.location = (-0.08, 5.2, 0.95)
    cam.rotation_euler = (Vector((-0.08, 0.0, 0.93)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 5.05
    scene.render.filepath = str(out_dir / "preview_long_hair_male_female_and_iso.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 1.0
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

    hair_obj = build_long_loose_hair_mesh(body_obj, arm_obj)
    verify_long_hair_fit(hair_obj)

    tex_png = OUT_DIR / "cabelo_longo_solto_128.png"
    uv_png = OUT_DIR / "cabelo_longo_solto_uv.png"
    glb_path = OUT_DIR / "CabeloLongoSolto.glb"
    blend_out = OUT_DIR / "cabelo_longo_solto.blend"

    tex_img = generate_psx_long_hair_texture(tex_png)
    export_uv_overlay(hair_obj, uv_png)
    export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path)

    hair_obj.parent = arm_obj
    hair_obj.parent_type = "BONE"
    hair_obj.parent_bone = "head"
    head_bone = arm_obj.pose.bones["head"]
    hair_obj.matrix_parent_inverse = (arm_obj.matrix_world @ head_bone.matrix).inverted()

    bpy.ops.wm.save_as_mainfile(filepath=str(blend_out))
    render_long_hair_previews(hair_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CabeloLongoSolto built & rendered successfully!")


if __name__ == "__main__":
    main()
