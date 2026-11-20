"""Build the first Grimora low-poly PSX hair model for chartest: Short Male Hair (CabeloCurtoMasculino).

Generates in the conversation deliverable directory (prior to user approval):
- CabeloCurtoMasculino.glb        (embedded 128x128 PSX hair texture, head-pivot hierarchy)
- cabelo_curto_masculino_128.png  (128x128 dark-fantasy PSX strand texture)
- cabelo_curto_masculino_uv.png   (128x128 transparent UV overlay for optional hand-painting)
- cabelo_curto_masculino.blend    (authoring scene on chartest for inspection)
- preview_hair_angles.png         (4-angle close-up on chartest head: Front, 3/4 Part, Side Profile, 3/4 Back)
- preview_hair_isolated_and_full.png (Isolated hair turnaround + Full-body shot on chartest)
"""

import json
import math
import pathlib
import struct
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree


CHARTEST_BLEND = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\chartest.blend"
)
OUT_DIR = pathlib.Path(
    r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\tools\blender\clothes\out\hair_short_male"
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


def ang_diff(a, b):
    """Shortest signed angle difference a - b in radians (-pi..pi)."""
    d = (a - b + math.pi) % (2.0 * math.pi) - math.pi
    return d


def slerp_dir(u0: Vector, u1: Vector, t: float) -> Vector:
    a = u0.normalized()
    b = u1.normalized()
    dot = clamp(a.dot(b), -0.999999, 0.999999)
    omega = math.acos(dot)
    if omega < 1e-5:
        return a.lerp(b, t).normalized()
    so = math.sin(omega)
    return ((math.sin((1.0 - t) * omega) / so) * a + (math.sin(t * omega) / so) * b).normalized()


def generate_psx_hair_texture(png_path: pathlib.Path, w: int = 128, h: int = 128):
    """Author a gritty, hand-pixelated 128x128 PSX dark-fantasy brown hair texture matching Grimora's grain.

    UV layout convention:
    - V in [0.09 .. 0.98]: radial hair cap & locks (V=0.98 = crown whorl root, V=0.09 = hairline/nape tips;
      U=0..1 wraps seamlessly around the head, with U~0.42 aligned to the left side-part).
    - V in [0.00 .. 0.07]: dark interior cap lining / scalp shadow.
    """
    px = [0.0] * (w * h * 4)

    def h2(ix, iy):
        n = (ix * 374761393 + iy * 668265263) & 0xFFFFFFFF
        n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
        return ((n ^ (n >> 16)) & 0xFFFF) / 65535.0

    def periodic_noise_1d(u_val: float, period: int, seed_y: int) -> float:
        """Seamless periodic 1D value noise over U in [0, 1) with integer period."""
        pos = (u_val % 1.0) * period
        i0 = int(math.floor(pos)) % period
        i1 = (i0 + 1) % period
        f = pos - math.floor(pos)
        t = f * f * (3.0 - 2.0 * f)
        return h2(i0, seed_y) * (1.0 - t) + h2(i1, seed_y) * t

    # Rich dark-fantasy umber/chestnut palette matching chartest's warm low-poly skin contrast
    root_col = (0.068, 0.046, 0.033)
    mid_col = (0.215, 0.144, 0.096)
    hi_col = (0.355, 0.248, 0.168)
    shadow_col = (0.042, 0.029, 0.021)

    for y in range(h):
        v = (y + 0.5) / h
        # Bottom strip V < 0.075 is pure dark scalp/underside shadow (prevents bright interior lining)
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

        # Quantize V in 2px painterly blocks for authentic PSX fibrous brushstroke feel
        vq = (int(y // 2) + 0.5) / (h / 2.0)

        for x in range(w):
            # Subtle organic wave along hair shaft so strands aren't ruler-straight vertical lines
            u_raw = (x + 0.5) / w
            wave = (periodic_noise_1d(u_raw, 16, int(y // 10) + 31) - 0.5) * 0.014
            u = (u_raw + wave) % 1.0

            # Multi-scale irregular seamless hair locks (NO uniform sine stripes!)
            broad_lock = periodic_noise_1d(u, 14, 3)          # irregular ~9px clumps
            med_lock = periodic_noise_1d(u, 32, 11)           # irregular ~4px locks
            fine_strand = periodic_noise_1d(u, 64, 23 + (y // 16))  # broken vertical fibers
            micro_stroke = h2(x, y // 3)                      # 1x3px pixel brushstrokes

            strand = (
                0.34 * broad_lock
                + 0.32 * med_lock
                + 0.22 * fine_strand
                + 0.12 * micro_stroke
            )

            # Per-lock staggered highlight height (breaks horizontal band into organic painterly strokes)
            hi_v_center = 0.58 + 0.16 * (periodic_noise_1d(u, 18, 47) - 0.5)
            crown_dark = smoothstep(0.84, 0.99, vq)
            tip_dark = smoothstep(0.22, 0.08, vq)
            spec_band = bell(vq, hi_v_center, 0.24) * smoothstep(0.42, 0.78, strand)

            # Subtle scalp shadow along the left side-part (u ~ 0.42)
            part_shadow = bell(u_raw, 0.422, 0.028) * smoothstep(0.18, 0.58, vq) * 0.42

            shade = 0.62 + 0.72 * strand
            r = mid_col[0] * shade + (hi_col[0] - mid_col[0]) * spec_band * 0.85
            g = mid_col[1] * shade + (hi_col[1] - mid_col[1]) * spec_band * 0.85
            b = mid_col[2] * shade + (hi_col[2] - mid_col[2]) * spec_band * 0.85

            dark_f = clamp(0.70 * crown_dark + 0.56 * tip_dark + part_shadow, 0.0, 0.90)
            r = r * (1.0 - dark_f) + root_col[0] * dark_f
            g = g * (1.0 - dark_f) + root_col[1] * dark_f
            b = b * (1.0 - dark_f) + root_col[2] * dark_f

            # Subtle PSX pixel dither + texel grain matching Chartest-Grimora-v02-128.png
            checker = 1.045 if ((x + y) & 1) == 0 else 0.955
            jitter = 0.94 + 0.12 * h2(x, y)
            r = clamp(r * checker * jitter, shadow_col[0], 0.48)
            g = clamp(g * checker * jitter, shadow_col[1], 0.36)
            b = clamp(b * checker * jitter, shadow_col[2], 0.26)

            idx = (y * w + x) * 4
            px[idx : idx + 4] = [r, g, b, 1.0]

    img = bpy.data.images.new("cabelo_curto_masculino_128", width=w, height=h, alpha=True)
    img.pixels[:] = px
    img.filepath_raw = str(png_path)
    img.file_format = "PNG"
    img.save()
    return img


def export_uv_overlay(mesh_obj, uv_png_path: pathlib.Path, size: int = 128):
    """Write a 128x128 transparent PNG with crisp 1px UV edges for optional user painting."""
    buf = [0.0] * (size * size * 4)

    def put_px(ix, iy, rgba):
        if 0 <= ix < size and 0 <= iy < size:
            o = (iy * size + ix) * 4
            buf[o : o + 4] = rgba

    def draw_line(u0, v0, u1, v1):
        x0, y0 = int(round(u0 * (size - 1))), int(round(v0 * (size - 1)))
        x1, y1 = int(round(u1 * (size - 1))), int(round(v1 * (size - 1)))
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
    """Build BVHTree of chartest_body's Basis head+neck surface for exact skull raycasting."""
    bm = bmesh.new()
    bm.from_mesh(body_obj.data)
    # Ensure we use Basis coordinates (un-morphed default head)
    if body_obj.data.shape_keys:
        basis = body_obj.data.shape_keys.key_blocks[0].data
        for i, v in enumerate(bm.verts):
            v.co = basis[i].co.copy()
    # Keep only Z >= 1.54m (neck & head)
    del_verts = [v for v in bm.verts if v.co.z < 1.53]
    bmesh.ops.delete(bm, geom=del_verts, context="VERTS")
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    bvh = BVHTree.FromBMesh(bm)
    bm.free()
    return bvh


def hairline_z(phi: float) -> float:
    """Anatomical masculine short-cut hairline height Z(phi) around C=(0, -0.075, 1.690).

    phi = 0     -> Center forehead (+Y)
    phi = +pi/2 -> Right sideburn/ear (+X)
    phi = -pi/2 -> Left sideburn/ear (-X)
    phi = +-pi  -> Back nape (-Y)
    """
    ap = abs(phi)
    deg = math.degrees(phi)
    adeg = abs(deg)

    # Base smooth profile from forehead -> temple -> sideburn -> ear arch -> mastoid -> nape
    # 1. Natural masculine forehead height (~1.737m..1.751m, ~20-32mm above the 1.718m brow ridge)
    z = 1.745

    # Subtle left side-part notch (-26 deg) + understated masculine temple corners (+-43 deg)
    z += 0.007 * bell(deg, -26.0, 14.0)
    z += 0.005 * bell(deg, +43.0, 12.0)
    # Broad, cohesive front-right swept fringe curve (frames forehead smoothly down to ~1.737m)
    z -= 0.008 * bell(deg, +8.0, 26.0)
    # Gentle left-brow framing curve (-42 deg)
    z -= 0.003 * bell(deg, -43.0, 11.0)

    # 2. Temple step descending toward sideburn (adeg 47..72) — keep active so transition into ear arch has zero upward spike!
    temple_drop = smoothstep(47.0, 72.0, adeg)
    z -= 0.028 * temple_drop

    # 3. Masculine sideburn ("costeleta") in front of ear: adeg in [70..89], y ~ -0.050..-0.074m!
    #    Note: ear starts at y = -0.0833m -> adeg >= 95.9 deg!
    sideburn = bell(adeg, 81.0, 10.0)
    z -= 0.036 * sideburn  # dips cleanly to z ~ 1.681m in front of the tragus!

    # 4. Snug Ear Cutout Arch: ear pinna spans adeg 95.9..108.9 (z_ear_top = 1.7065m).
    #    Arch smoothly at z ~ 1.710..1.714m right over the ear, then drop IMMEDIATELY behind the ear!
    ear_arch = smoothstep(87.0, 94.0, adeg) * (1.0 - smoothstep(108.5, 112.0, adeg))
    z = z * (1.0 - ear_arch) + (1.710 + 0.004 * bell(adeg, 101.0, 9.0)) * ear_arch

    # 5. Immediate descent right behind the ear (adeg 109.5 -> 126 fills the post-auricular area!)
    #    followed by a smooth taper into the masculine nape (z ~ 1.615m)
    post_ear_drop = smoothstep(109.0, 125.0, adeg)
    nape_settle = smoothstep(123.0, 156.0, adeg)
    z_behind_ear = 1.624 * (1.0 - nape_settle) + (1.616 - 0.003 * math.cos(adeg * math.radians(3.0))) * nape_settle
    z = z * (1.0 - post_ear_drop) + z_behind_ear * post_ear_drop

    return z


def skull_raycast(bvh: BVHTree, origin: Vector, direction: Vector, fallback_r: float = 0.095):
    d = direction.normalized()
    loc, norm, _, _ = bvh.ray_cast(origin, d, 0.35)
    if loc is not None:
        return loc, (norm if norm.dot(d) > 0 else -norm)
    # Fallback reverse cast from outside inward in case origin is near grazing angle
    far = origin + d * 0.30
    loc2, norm2, _, _ = bvh.ray_cast(far, -d, 0.35)
    if loc2 is not None:
        return loc2, (norm2 if norm2.dot(d) > 0 else -norm2)
    return origin + d * fallback_r, d


def build_short_male_hair_mesh(body_obj) -> bpy.types.Object:
    """Sculpt faceted dark-fantasy PSX Short Male Hair directly fitted to chartest's skull."""
    bvh = build_head_bvh(body_obj)

    # Cranial reference center (aligned so ear sits cleanly at |phi| ~ 95..112 deg)
    C = Vector((0.0, -0.075, 1.690))

    # Off-center crown whorl ("redemoinho") on upper-back-left of cranium
    whorl_dir = Vector((-0.013, -0.028, 0.128)).normalized()
    whorl_skull, whorl_n = skull_raycast(bvh, C, whorl_dir)
    whorl_outer = whorl_skull + whorl_n * 0.0175

    # Azimuthal resolution: 40 columns around the head, with the +-108 deg column aligned to +-109.5 deg
    # right behind the upper-back ear tip (y=-0.1027m) so the hairline drops cleanly behind the ear!
    N_AZ = 40
    # Radial rings from whorl (j=0) to outer hairline edge (j=N_RINGS) + tucked inner scalp lip
    s_rings = [0.16, 0.34, 0.54, 0.75, 0.91, 1.00]

    bm = bmesh.new()
    uv_lay = bm.loops.layers.uv.new("UVMap")

    # Precompute hairline target directions & skull points for each azimuth column
    phis = []
    for k in range(N_AZ):
        raw_phi = -math.pi + (2.0 * math.pi * k) / N_AZ
        raw_deg = math.degrees(raw_phi)
        sign = 1.0 if raw_deg >= 0.0 else -1.0
        # Align +-108 deg column to +-109.6 deg (2mm behind ear helix tip) for a razor-clean post-ear drop
        warped_deg = raw_deg + sign * 1.6 * bell(abs(raw_deg), 108.0, 8.0)
        phis.append(math.radians(warped_deg))
    hl_dirs = []
    for phi in phis:
        zh = hairline_z(phi)
        # Cast horizontally from spine/head axis at height zh to find exact skull perimeter point!
        axis_pt = Vector((0.0, -0.075, clamp(zh, 1.615, 1.775)))
        horiz_dir = Vector((math.sin(phi), math.cos(phi), 0.0)).normalized()
        p_hit, _ = skull_raycast(bvh, axis_pt, horiz_dir, fallback_r=0.082)
        p_target = Vector((p_hit.x, p_hit.y, zh))
        hl_dirs.append((p_target - C).normalized())

    # Create central whorl vertex
    v_center = bm.verts.new(whorl_outer)

    ring_verts = []  # list of lists [N_AZ]
    inner_lip_verts = []

    for r_idx, s in enumerate(s_rings):
        row = []
        for k, phi in enumerate(phis):
            deg = math.degrees(phi)
            adeg = abs(deg)

            # Natural comb sweep: top-front hair sweeps gently from left part (-26 deg) toward right (+X)
            sweep_bias = 0.0
            if adeg < 62.0:
                part_side = smoothstep(-24.0, -8.0, deg)  # right of part sweeps right
                sweep_bias = (0.10 * part_side - 0.04 * (1.0 - part_side)) * math.sin(s * math.pi) * (1.0 - smoothstep(38.0, 62.0, adeg))

            base_dir = slerp_dir(whorl_dir, hl_dirs[k], s)
            if abs(sweep_bias) > 1e-5:
                tangent = Vector((math.cos(phi), -math.sin(phi), 0.0))
                base_dir = (base_dir + tangent * sweep_bias).normalized()

            p_sk, n_sk = skull_raycast(bvh, C, base_dir)
            out_dir = (base_dir * 0.65 + n_sk * 0.35).normalized()

            # --- Masculine Short Cut Volume Profile (fuller/textured on top, tapered on sides & nape!) ---
            topness = clamp((base_dir.z + 0.05) / 0.95, 0.0, 1.0)
            frontness = smoothstep(85.0, 0.0, adeg)
            sideness = bell(adeg, 92.0, 38.0)
            napeness = smoothstep(120.0, 175.0, adeg)

            # Base thickness in meters: ~21mm on top/front, ~10.5mm on upper sides, ~9mm down nape
            thick = 0.0110 + 0.0105 * (topness ** 0.85) + 0.0045 * frontness * smoothstep(0.25, 0.85, s)
            thick -= 0.0028 * sideness * smoothstep(0.45, 1.0, s)
            thick -= 0.0025 * napeness * smoothstep(0.55, 1.0, s)

            # Left side-part crease (real 3D groove along deg ~ -26) + raised swept crest on right of part!
            part_groove = bell(deg, -26.5, 9.5) * smoothstep(0.20, 0.70, s)
            swept_crest = bell(deg, +6.0, 26.0) * smoothstep(0.25, 0.88, s)
            thick -= 0.0052 * part_groove
            thick += 0.0048 * swept_crest

            # Faceted PSX lock clumping ridges (subtle angular hair planes catching light!)
            clump_wave = math.cos(phi * 7.0 - 0.45) * 0.0020 * math.sin(s * math.pi * 0.92)
            # Nape V-taper ridges on back of head
            nape_ridge = math.cos(phi * 9.0) * 0.0016 * napeness * bell(s, 0.72, 0.28)
            thick += clump_wave + nape_ridge

            # Crisp beveled taper at the outer hairline ring (s=1.0), while keeping front fringe proud!
            if r_idx == len(s_rings) - 1:
                # At s=1.0: front fringe keeps ~8.5mm step for sculpted brow-framing volume;
                # sideburns & nape taper cleanly to ~3.2mm against skin!
                edge_keep = 0.34 + 0.36 * frontness * (1.0 - 0.55 * part_groove)
                thick *= edge_keep
                # Ensure ear arch clears z_ear_top (1.7065m) strictly over the ear (94..109.8 deg)
                if 94.0 <= adeg <= 109.8:
                    p_sk.z = max(p_sk.z, 1.710)
            elif r_idx == len(s_rings) - 2:
                thick *= 0.88 + 0.10 * frontness

            thick = max(0.0032, thick)
            p_hair = p_sk + out_dir * thick
            row.append(bm.verts.new(p_hair))

            # Also build the watertight inner scalp lip right inside the skull at s=1.0
            if r_idx == len(s_rings) - 1:
                lip_pt = p_sk - out_dir * 0.0018 + Vector((0.0, 0.0, 0.0015))
                if 94.0 <= adeg <= 109.8:
                    lip_pt.z = max(lip_pt.z, 1.7095)
                inner_lip_verts.append(bm.verts.new(lip_pt))

        ring_verts.append(row)

    # --- Build Cap Faces + Radial Strand UVs (Seam cleanly down center-back nape k=0 / k=N_AZ) ---
    def u_of(k_idx):
        return k_idx / N_AZ

    def v_of_ring(r_idx):
        # Crown whorl = 0.96, outer hairline = 0.10
        s = s_rings[r_idx]
        return 0.96 - 0.86 * s

    # 1. Innermost triangle fan around crown whorl
    r0 = ring_verts[0]
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((v_center, r0[k], r0[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (0.5 * (u0 + u1), 0.985)
        f.loops[1][uv_lay].uv = (u0, v_of_ring(0))
        f.loops[2][uv_lay].uv = (u1, v_of_ring(0))

    # 2. Concentric quad strips down to the hairline
    for r_idx in range(len(s_rings) - 1):
        ra = ring_verts[r_idx]
        rb = ring_verts[r_idx + 1]
        va, vb = v_of_ring(r_idx), v_of_ring(r_idx + 1)
        for k in range(N_AZ):
            kn = (k + 1) % N_AZ
            f = bm.faces.new((ra[k], rb[k], rb[kn], ra[kn]))
            u0, u1 = u_of(k), u_of(k + 1)
            f.loops[0][uv_lay].uv = (u0, va)
            f.loops[1][uv_lay].uv = (u0, vb)
            f.loops[2][uv_lay].uv = (u1, vb)
            f.loops[3][uv_lay].uv = (u1, va)

    # 3. Hairline thickness bevel / tucked scalp rim + watertight dark inner lining dome
    r_last = ring_verts[-1]
    v_last = v_of_ring(len(s_rings) - 1)
    v_inner_crown = bm.verts.new(whorl_skull - whorl_n * 0.0025)
    for k in range(N_AZ):
        kn = (k + 1) % N_AZ
        f = bm.faces.new((r_last[k], inner_lip_verts[k], inner_lip_verts[kn], r_last[kn]))
        u0, u1 = u_of(k), u_of(k + 1)
        f.loops[0][uv_lay].uv = (u0, v_last)
        f.loops[1][uv_lay].uv = (u0, 0.04)
        f.loops[2][uv_lay].uv = (u1, 0.04)
        f.loops[3][uv_lay].uv = (u1, v_last)

        # Seal interior cap with downward/inward normals mapped to dark shadow strip (V=0.03)
        f_in = bm.faces.new((inner_lip_verts[kn], inner_lip_verts[k], v_inner_crown))
        for lp in f_in.loops:
            lp[uv_lay].uv = (0.5, 0.03)

    # --- Add Broad Chiseled Low-Poly Layered Locks (Swept Forelocks + Crown Tuft) ---
    def add_sculpted_lock(spine_pts, widths, heights, u_center=0.52):
        """Build a closed, broad chiseled PSX hair lock band (no spiky needle tips)."""
        n_seg = len(spine_pts)
        sec_verts = []
        for idx, pt in enumerate(spine_pts):
            n_out = (pt - C).normalized()
            if idx < n_seg - 1:
                tang = (spine_pts[idx + 1] - pt).normalized()
            else:
                tang = (pt - spine_pts[idx - 1]).normalized()
            side = tang.cross(n_out).normalized()
            w_half = widths[idx] * 0.5
            h_ridge = heights[idx]
            sink = 0.0022 if idx < n_seg - 1 else 0.0012
            v_l = bm.verts.new(pt - side * w_half - n_out * sink)
            v_r = bm.verts.new(pt + side * w_half - n_out * sink)
            v_top = bm.verts.new(pt + n_out * h_ridge)
            sec_verts.append((v_l, v_top, v_r))

        # Cap root & broad beveled front edge
        r_l, r_top, r_r = sec_verts[0]
        f_root = bm.faces.new((r_l, r_r, r_top))
        for lp in f_root.loops:
            lp[uv_lay].uv = (u_center, 0.80)

        t_l, t_top, t_r = sec_verts[-1]
        f_tip = bm.faces.new((t_l, t_top, t_r))
        f_tip.loops[0][uv_lay].uv = (u_center - 0.05, 0.11)
        f_tip.loops[1][uv_lay].uv = (u_center, 0.12)
        f_tip.loops[2][uv_lay].uv = (u_center + 0.05, 0.11)

        du = 0.082  # Matches main cap texel density so pixel fibers stay sharp and cohesive!
        for s_i in range(n_seg - 1):
            v0 = 0.82 - 0.70 * (s_i / (n_seg - 1))
            v1 = 0.82 - 0.70 * ((s_i + 1) / (n_seg - 1))
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
            f2.loops[2][u_lay_tmp := uv_lay].uv = (u_center + du, v1)
            f2.loops[3][uv_lay].uv = (u_center + du, v0)

            f3 = bm.faces.new((cur[2], nxt[2], nxt[0], cur[0]))
            for lp in f3.loops:
                lp[uv_lay].uv = (u_center, 0.03)

    def surf_pt(x, y, z, lift=0.016):
        d = (Vector((x, y, z)) - C).normalized()
        p_sk, n_sk = skull_raycast(bvh, C, d)
        return p_sk + (d * 0.65 + n_sk * 0.35).normalized() * lift

    # Broad, cohesive sculpted volume locks integrated smoothly into the swept fringe
    # Lock 1: Primary swept front fringe band flowing from left part across center-right brow
    add_sculpted_lock(
        [
            surf_pt(-0.016, -0.022, 1.815, 0.021),
            surf_pt(+0.002, +0.010, 1.792, 0.024),
            surf_pt(+0.016, +0.034, 1.764, 0.019),
            surf_pt(+0.022, +0.044, 1.741, 0.010),
        ],
        widths=[0.042, 0.050, 0.042, 0.024],
        heights=[0.0035, 0.0062, 0.0055, 0.0025],
        u_center=0.52,
    )

    # Lock 2: Secondary right-swept lock blending into the right temple (eliminates temple V-gap!)
    add_sculpted_lock(
        [
            surf_pt(+0.014, -0.026, 1.810, 0.019),
            surf_pt(+0.034, +0.005, 1.786, 0.021),
            surf_pt(+0.047, +0.024, 1.761, 0.016),
            surf_pt(+0.053, +0.031, 1.746, 0.009),
        ],
        widths=[0.038, 0.042, 0.034, 0.020],
        heights=[0.0030, 0.0050, 0.0044, 0.0020],
        u_center=0.60,
    )

    # Lock 3: Subtle left-side swept lock framing the left side of the part
    add_sculpted_lock(
        [
            surf_pt(-0.028, -0.024, 1.808, 0.018),
            surf_pt(-0.040, +0.006, 1.784, 0.019),
            surf_pt(-0.048, +0.024, 1.761, 0.015),
            surf_pt(-0.051, +0.031, 1.746, 0.008),
        ],
        widths=[0.032, 0.036, 0.030, 0.018],
        heights=[0.0028, 0.0045, 0.0038, 0.0018],
        u_center=0.35,
    )

    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])

    mesh = bpy.data.meshes.new("CabeloCurtoMasculino")
    bm.to_mesh(mesh)
    bm.free()

    # Crisp PSX low-poly shading: flat/auto-smooth crispness on lock creases, smooth cap flow
    for poly in mesh.polygons:
        poly.use_smooth = False

    hair_obj = bpy.data.objects.new("CabeloCurtoMasculino", mesh)
    bpy.context.scene.collection.objects.link(hair_obj)
    return hair_obj


def verify_hair_fit(hair_obj, body_obj):
    """Verify triangle budget, ear clearance, brow clearance, and skull coverage."""
    bm = bmesh.new()
    bm.from_mesh(hair_obj.data)
    bmesh.ops.triangulate(bm, faces=bm.faces[:])
    tri_count = len(bm.faces)
    verts = [v.co.copy() for v in bm.verts]
    bm.free()

    # Check ear box clearance: ear pinna spans y in [-0.1027, -0.0833], |x| > 0.078, z_max = 1.7065m
    ear_zone = [p for p in verts if -0.1028 <= p.y <= -0.0825 and abs(p.x) > 0.078]
    min_ear_z = min((p.z for p in ear_zone), default=9.9)
    behind_ear_zone = [p for p in verts if -0.132 <= p.y <= -0.106 and abs(p.x) > 0.060]
    min_behind_ear_z = min((p.z for p in behind_ear_zone), default=9.9)

    # Check forehead/brow clearance: brow ridge is z=1.718m in y > 0.030, |x| < 0.045
    brow_zone = [p for p in verts if p.y > 0.032 and abs(p.x) < 0.045]
    min_brow_z = min((p.z for p in brow_zone), default=9.9)

    print(f"[hair-verify] triangles={tri_count}, verts={len(verts)}")
    print(f"[hair-verify] ear_arch_min_z={min_ear_z:.4f}m (ear top=1.7065m -> clearance={(min_ear_z - 1.7065)*1000:.1f}mm)")
    print(f"[hair-verify] behind_ear_min_z={min_behind_ear_z:.4f}m (fills post-auricular zone down past 1.65m earlobe!)")
    print(f"[hair-verify] forehead_min_z={min_brow_z:.4f}m (brow=1.7180m -> clearance={(min_brow_z - 1.718)*1000:.1f}mm)")
    assert min_ear_z >= 1.7085, f"Hair encroaches on ear! min_ear_z={min_ear_z}"
    assert min_brow_z >= 1.732, f"Hair hangs too low over brow! min_brow_z={min_brow_z}"
    return tri_count


def export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path: pathlib.Path):
    """Export CabeloCurtoMasculino.glb with head-bone socket pivot & embedded 128x128 PNG."""
    head_pivot = arm_obj.data.bones["head"].head_local.copy()

    # Create a clean temporary root/pivot empty at `head` bone pivot so:
    # - World position matches chartest standing at (0,0,0) automatically!
    # - Local mesh origin is relative to `head` bone pivot for 1:1 socket attachment!
    pivot_empty = bpy.data.objects.new("head_socket", None)
    bpy.context.scene.collection.objects.link(pivot_empty)
    pivot_empty.location = head_pivot

    export_copy = hair_obj.copy()
    export_copy.data = hair_obj.data.copy()
    export_copy.name = "CabeloCurtoMasculino"
    # Shift vertices into `head` bone local coordinates while placing object under `head_socket`
    export_copy.data.transform(Matrix.Translation(-head_pivot))
    export_copy.parent = pivot_empty
    export_copy.matrix_parent_inverse = Matrix.Identity(4)
    export_copy.location = (0.0, 0.0, 0.0)
    bpy.context.scene.collection.objects.link(export_copy)

    # Material matching Grimora dynamic character PBR (128x128 embedded PNG)
    mat = bpy.data.materials.new("cabelo_curto_masculino")
    mat.use_nodes = True
    mat.use_backface_culling = False
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.inputs["Roughness"].default_value = 0.88
    bsdf.inputs["Metallic"].default_value = 0.0
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.12
    tex = nodes.new("ShaderNodeTexImage")
    tex.image = tex_img
    tex.interpolation = "Closest"  # Match chartest's crisp PSX pixel texels!
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

    # Cleanup temp export wrapper
    tmp_mesh = export_copy.data
    bpy.data.objects.remove(export_copy, do_unlink=True)
    bpy.data.objects.remove(pivot_empty, do_unlink=True)
    bpy.data.meshes.remove(tmp_mesh)

    # Validate GLB container & embedded image
    raw = glb_path.read_bytes()
    magic, version, total_len = struct.unpack_from("<4sII", raw, 0)
    assert magic == b"glTF" and version == 2 and total_len == len(raw)
    jlen = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20 : 20 + jlen])
    print(f"[glb-verify] {glb_path.name} ({len(raw)} bytes), images={len(doc.get('images', []))}, meshes={len(doc.get('meshes', []))}")


def render_hair_previews(hair_obj, body_obj, arm_obj, out_dir: pathlib.Path):
    """Render high-clarity multi-angle head close-ups + isolated & full-body shots."""
    scene = bpy.context.scene
    for t in arm_obj.animation_data.nla_tracks:
        t.mute = True
    arm_obj.animation_data.action = None

    # Use Nearest/Closest on body & hair for crisp PSX pixel showcase clarity, or standard Grimora filtering
    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"

    world = bpy.data.worlds.new("HairPreviewWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.055, 0.064, 0.080, 1.0)
    bg.inputs[1].default_value = 1.0

    lights = []
    for lname, loc, energy, col in [
        ("HKey", (-2.0, 3.6, 3.4), 4.4, (1.0, 0.97, 0.92)),
        ("HFill", (2.8, 2.6, 2.2), 2.2, (0.82, 0.88, 1.0)),
        ("HRim", (0.2, -3.4, 2.8), 2.8, (0.95, 0.96, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy = energy
        ldata.color = col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.72)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
        lights.append(lobj)

    cam_data = bpy.data.cameras.new("HairCam")
    cam = bpy.data.objects.new("HairCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    cam_data.type = "ORTHO"

    depsgraph = bpy.context.evaluated_depsgraph_get()
    body_obj.hide_render = True
    arm_obj.hide_render = True
    hair_obj.hide_render = True

    def spawn_pair(x_pos, yaw_deg, include_body=True, height_m=1.82):
        arm_obj["height_m"] = height_m
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

    # --- SHEET 1: 4-Angle Head Close-up on chartest (Front 0°, 3/4 Part +38°, Pure Side -90°, 3/4 Back -142°)
    snaps = []
    for x_p, yaw in [(0.54, 0.0), (0.18, 38.0), (-0.18, -90.0), (-0.54, -142.0)]:
        snaps.extend(spawn_pair(x_p, yaw, include_body=True))

    scene.render.resolution_x = 1600
    scene.render.resolution_y = 680
    cam.location = (0.0, 3.0, 1.72)
    cam.rotation_euler = (Vector((0.0, 0.0, 1.70)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 1.48
    scene.render.filepath = str(out_dir / "preview_hair_angles.png")
    bpy.ops.render.render(write_still=True)
    cleanup_objs(snaps)

    # --- SHEET 2: Full-Body Proportion Check + Isolated Hair Asset Views
    snaps = []
    # Full body front-3/4 & back-3/4 on left, plus zoomed isolated hair piece on right (lifted to center Z~0.95)
    snaps.extend(spawn_pair(1.55, -24.0, include_body=True))
    snaps.extend(spawn_pair(0.48, -135.0, include_body=True))
    for x_p, yaw in [(-0.62, 32.0), (-1.58, -125.0)]:
        iso = spawn_pair(x_p, yaw, include_body=False)
        for o in iso:
            o.scale = (2.6, 2.6, 2.6)
            o.location = (x_p, 0.0, 1.02 - 1.72 * 2.6)
        snaps.extend(iso)

    scene.render.resolution_x = 1560
    scene.render.resolution_y = 800
    cam.location = (0.0, 5.0, 0.96)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 4.45
    scene.render.filepath = str(out_dir / "preview_hair_isolated_and_full.png")
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

    # Ensure clean default A-pose & default male 1.82m head during authoring
    for t in arm_obj.animation_data.nla_tracks:
        t.mute = True
    arm_obj.animation_data.action = None
    arm_obj["is_female"] = False
    arm_obj["height_m"] = 1.82

    hair_obj = build_short_male_hair_mesh(body_obj)
    verify_hair_fit(hair_obj, body_obj)

    tex_png = OUT_DIR / "cabelo_curto_masculino_128.png"
    uv_png = OUT_DIR / "cabelo_curto_masculino_uv.png"
    glb_path = OUT_DIR / "CabeloCurtoMasculino.glb"
    blend_out = OUT_DIR / "cabelo_curto_masculino.blend"

    tex_img = generate_psx_hair_texture(tex_png)
    export_uv_overlay(hair_obj, uv_png)
    export_standalone_glb(hair_obj, arm_obj, tex_img, glb_path)

    # Parent hair to chartest_rig -> `head` bone in the review .blend so head rotations carry the hair!
    hair_obj.parent = arm_obj
    hair_obj.parent_type = "BONE"
    hair_obj.parent_bone = "head"
    head_bone = arm_obj.pose.bones["head"]
    hair_obj.matrix_parent_inverse = (arm_obj.matrix_world @ head_bone.matrix).inverted()

    bpy.ops.wm.save_as_mainfile(filepath=str(blend_out))
    render_hair_previews(hair_obj, body_obj, arm_obj, OUT_DIR)
    print("[done] CabeloCurtoMasculino built & rendered successfully!")


if __name__ == "__main__":
    main()
