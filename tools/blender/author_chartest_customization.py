"""Author full anatomical Shape Keys and proportional Height controller (0.40m - 2.60m) on chartest.

Operates on the upgraded 1,834-quad `assets/models/chartest/chartest.blend` and exports
standard glTF 2.0 morph targets + `extras.height_controller` to `chartest.gltf`,
preserving all 42 bones, sockets, right-hand grip clearance, and animations.

Run with Blender 5.2:
    blender --background --python tools/blender/author_chartest_customization.py -- --preview-dir <dir>
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import shutil
import struct
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

ROOT = Path(__file__).resolve().parents[2]
CHARTEST_DIR = ROOT / "assets/models/chartest"
BACKUP_DIR = CHARTEST_DIR / "authoring_archive/before_subdiv"


def smoothstep(edge0, edge1, x):
    if abs(edge1 - edge0) < 1e-9:
        return 1.0 if x >= edge1 else 0.0
    t = max(0.0, min(1.0, (x - edge0) / (edge1 - edge0)))
    return t * t * (3.0 - 2.0 * t)


def bell(x, center, half_width):
    d = abs(x - center) / max(half_width, 1e-6)
    if d >= 1.0:
        return 0.0
    u = 1.0 - d * d
    return u * u


def asym_bell(x, low, peak, high):
    if x <= low or x >= high:
        return 0.0
    if x < peak:
        return smoothstep(low, peak, x)
    return 1.0 - smoothstep(peak, high, x)


def project_segment(p, a, b):
    ab = b - a
    len_sq = ab.length_squared
    if len_sq < 1e-10:
        return 0.0, a.copy(), p - a
    t = (p - a).dot(ab) / len_sq
    t_clamped = max(0.0, min(1.0, t))
    closest = a + ab * t_clamped
    return t, closest, p - closest


def clear_customization(obj, arm):
    """Reset any prior shape keys or height drivers so reruns are 100% deterministic."""
    if obj.data.shape_keys:
        obj.shape_key_clear()
    if arm.animation_data and arm.animation_data.drivers:
        for fc in list(arm.animation_data.drivers):
            arm.animation_data.drivers.remove(fc)
    for pb in arm.pose.bones:
        pb.matrix_basis.identity()
        pb.location = Vector((0.0, 0.0, 0.0))
        pb.scale = Vector((1.0, 1.0, 1.0))
    for prop_name in ("height_m", "is_female", "female_bust", "breasts_size", "glutes_size"):
        if prop_name in arm:
            del arm[prop_name]
    for mat in bpy.data.materials:
        if mat.use_nodes and mat.node_tree and mat.node_tree.animation_data:
            mat.node_tree.animation_data_clear()


def build_vertex_adjacency(mesh):
    adj = defaultdict(set)
    for poly in mesh.polygons:
        verts = poly.vertices
        n = len(verts)
        for i in range(n):
            u = verts[i]
            v = verts[(i + 1) % n]
            adj[u].add(v)
            adj[v].add(u)
    return adj


def smooth_delta_field(deltas, adj, lock_weights, passes=2, strength=0.42):
    """Smooth displacement vectors across the quad manifold while preserving locked zones."""
    cur = [d.copy() for d in deltas]
    for _ in range(passes):
        nxt = []
        for i, d in enumerate(cur):
            keep = 1.0 - lock_weights[i]
            if keep <= 1e-4 or not adj[i]:
                nxt.append(d * keep)
                continue
            avg = Vector((0.0, 0.0, 0.0))
            for nb in adj[i]:
                avg += cur[nb]
            avg /= len(adj[i])
            nxt.append(d.lerp(avg, strength) * keep)
        cur = nxt
    return cur


def get_group_weights(obj):
    names = {g.index: g.name for g in obj.vertex_groups}
    per_vert = []
    for v in obj.data.vertices:
        wmap = defaultdict(float)
        for g in v.groups:
            if g.group in names:
                wmap[names[g.group]] = g.weight
        per_vert.append(wmap)
    return per_vert


def add_shape_key(obj, name, deltas, slider_min=0.0, slider_max=1.0):
    sk = obj.shape_key_add(name=name, from_mix=False)
    sk.slider_min = slider_min
    sk.slider_max = slider_max
    sk.value = 0.0
    basis = obj.data.shape_keys.key_blocks[0]
    for i, delta in enumerate(deltas):
        sk.data[i].co = basis.data[i].co + delta
    return sk


def author_anatomical_shape_keys(obj, arm):
    """Create comprehensive, organic anatomical Shape Keys on chartest_body."""
    mesh = obj.data
    mesh.update()
    verts = [v.co.copy() for v in mesh.vertices]
    normals = [v.normal.copy() for v in mesh.vertices]
    vweights = get_group_weights(obj)
    adj = build_vertex_adjacency(mesh)

    bones = {b.name: (b.head_local.copy(), b.tail_local.copy()) for b in arm.data.bones}
    # True joint-to-joint segment endpoints in armature/mesh bind space (Z-up, +Y forward, +X right)
    segs = {
        "upper_arm.L": (bones["upper_arm.L"][0], bones["forearm.L"][0]),
        "upper_arm.R": (bones["upper_arm.R"][0], bones["forearm.R"][0]),
        "forearm.L": (bones["forearm.L"][0], bones["hand.L"][0]),
        "forearm.R": (bones["forearm.R"][0], bones["hand.R"][0]),
        "thigh.L": (bones["thigh.L"][0], bones["shin.L"][0]),
        "thigh.R": (bones["thigh.R"][0], bones["shin.R"][0]),
        "shin.L": (bones["shin.L"][0], bones["foot.L"][0]),
        "shin.R": (bones["shin.R"][0], bones["foot.R"][0]),
        "neck": (bones["neck"][0], bones["head"][0]),
    }

    # Strict protection mask: hands/fingers stay 100% locked (0 delta) so sword/book grip tests
    # NEVER drift under any body customization slider; foot soles lock Z at ground plane.
    hand_bone_keys = {
        g.name for g in obj.vertex_groups
        if any(k in g.name.lower() for k in ("hand", "thumb", "index", "finger"))
    }
    lock_weights = []
    for i, p in enumerate(verts):
        wh = min(1.0, sum(vweights[i].get(k, 0.0) for k in hand_bone_keys))
        wf = min(1.0, vweights[i].get("foot.L", 0.0) + vweights[i].get("foot.R", 0.0)
                 + vweights[i].get("toe.L", 0.0) + vweights[i].get("toe.R", 0.0))
        sole_lock = 1.0 - smoothstep(0.012, 0.075, p.z)
        lock_weights.append(max(wh, wf * 0.85, sole_lock))

    def w_sum(i, *group_names):
        return min(1.0, sum(vweights[i].get(g, 0.0) for g in group_names))

    # Basis key
    obj.shape_key_add(name="Basis", from_mix=False)

    # Precompute limb segment projections for every vertex
    limb_info = {}
    for seg_name, (a, b) in segs.items():
        limb_info[seg_name] = [project_segment(p, a, b) for p in verts]

    def make_key(name, fn, smin=0.0, smax=1.0, smooth_passes=2, smooth_strength=0.40):
        raw = []
        for i, p in enumerate(verts):
            d = fn(i, p, normals[i])
            if p.z < 0.03:
                d.z *= smoothstep(0.005, 0.03, p.z)
            raw.append(d)
        smoothed = smooth_delta_field(raw, adj, lock_weights, passes=smooth_passes, strength=smooth_strength)
        # Prevent inner thigh self-intersection across X = 0
        for i, p in enumerate(verts):
            if 0.42 < p.z < 0.88 and abs(p.x) > 0.012:
                new_x = p.x + smoothed[i].x
                min_gap = 0.016
                if p.x > 0 and new_x < min_gap:
                    smoothed[i].x = min_gap - p.x
                elif p.x < 0 and new_x > -min_gap:
                    smoothed[i].x = -min_gap - p.x
        add_shape_key(obj, name, smoothed, slider_min=smin, slider_max=smax)

    # -------------------------------------------------------------------------
    # 1. LIMB & SEGMENT KEYS (Bidirectional: thin/slim <-> muscular/thick)
    # -------------------------------------------------------------------------

    # Shoulders_Width: Lateral deltoid cap + acromion breadth
    def fn_shoulders(i, p, n):
        w = w_sum(i, "clavicle.L", "clavicle.R", "upper_arm.L", "upper_arm.R", "chest")
        if w < 0.02 or p.z < 1.24 or p.z > 1.60:
            return Vector()
        z_mask = asym_bell(p.z, 1.25, 1.46, 1.58)
        lat = smoothstep(0.07, 0.21, abs(p.x))
        sx = 1.0 if p.x >= 0 else -1.0
        # Deltoid cap around shoulder head (~|x|=0.19, z=1.45)
        delt_l = limb_info["upper_arm.L"][i]
        delt_r = limb_info["upper_arm.R"][i]
        t_arm, _, rvec = delt_r if p.x >= 0 else delt_l
        arm_w = w_sum(i, "upper_arm.L", "upper_arm.R", "clavicle.L", "clavicle.R")
        cap = asym_bell(t_arm, -0.25, 0.14, 0.52) * arm_w
        rn = rvec.normalized() if rvec.length > 1e-5 else n
        return Vector((
            sx * (0.032 * z_mask * lat + 0.014 * cap),
            rn.y * 0.018 * cap,
            0.012 * z_mask * lat + max(0.0, rn.z) * 0.014 * cap,
        )) * w

    make_key("Shoulders_Width", fn_shoulders, smin=-0.65, smax=1.0)

    # Traps_Neck: Neck thickness + upper trapezius slope
    def fn_traps_neck(i, p, n):
        w_nk = w_sum(i, "neck", "head")
        w_tr = w_sum(i, "neck", "clavicle.L", "clavicle.R", "chest")
        d = Vector()
        # Neck cylinder radial expansion (below jaw/chin)
        if w_nk > 0.02 and 1.48 < p.z < 1.63:
            jaw_guard = 1.0 - smoothstep(0.00, 0.055, p.y) * smoothstep(1.56, 1.61, p.z)
            nk_mask = asym_bell(p.z, 1.48, 1.55, 1.625) * jaw_guard
            radial = Vector((p.x, p.y + 0.075, 0.0))
            if radial.length > 1e-5:
                rn = radial.normalized()
                d += Vector((rn.x * 0.018, rn.y * 0.016, 0.0)) * nk_mask * w_nk
        # Upper trapezius ridge between neck and acromion
        if w_tr > 0.02 and 1.42 < p.z < 1.58:
            trap_x = bell(abs(p.x), 0.105, 0.095)
            trap_z = asym_bell(p.z, 1.42, 1.515, 1.58)
            back_bias = smoothstep(0.04, -0.12, p.y)
            d += Vector((0.0, -0.012 * back_bias, 0.026 * (0.45 + 0.55 * back_bias))) * trap_x * trap_z * w_tr
        return d

    make_key("Traps_Neck", fn_traps_neck, smin=-0.65, smax=1.0)

    # Chest_Pecs: Male/athletic pectoral plates + ribcage depth (preserves sternum valley)
    def fn_chest_pecs(i, p, n):
        w = w_sum(i, "chest", "spine", "clavicle.L", "clavicle.R")
        if w < 0.02 or p.z < 1.15 or p.z > 1.55:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        rib_z = asym_bell(p.z, 1.15, 1.37, 1.53)
        pec_z = asym_bell(p.z, 1.25, 1.365, 1.49)
        # Sternal valley keeps x~0 indented between left/right pecs
        pec_x = asym_bell(abs(p.x), 0.012, 0.082, 0.18)
        front = smoothstep(-0.03, 0.045, p.y)
        back = smoothstep(0.00, -0.09, p.y)
        return Vector((
            sx * 0.026 * rib_z * smoothstep(0.03, 0.14, abs(p.x)),
            0.050 * pec_z * pec_x * front + 0.015 * rib_z * front - 0.020 * rib_z * back,
            0.008 * pec_z * pec_x * front,
        )) * w

    make_key("Chest_Pecs", fn_chest_pecs, smin=-0.60, smax=1.0)

    # Female_Breasts: Convex, round, weighted teardrop spanning z=1.242..1.455 (bigger max & rounder upper-front arc):
    #   Row 1 (1.451): clavicle transition
    #   Row 2 (1.406) & Row 3 (1.362): convex outward upper-front arch (never a flat diagonal ramp!)
    #   Row 4 (1.329): full round front-lower equator
    #   Row 5 (1.296): heavy rounded underside curving BACK UNDERNEATH Row 4 over Row 6
    #   Row 6 (1.242): stationary inframammary crease on ribcage
    def fn_female_breasts(i, p, n):
        w_arm = w_sum(i, "upper_arm.L", "upper_arm.R", "forearm.L", "forearm.R")
        w = w_sum(i, "chest", "spine", "clavicle.L", "clavicle.R") * (1.0 - smoothstep(0.16, 0.45, w_arm))
        if w < 0.02 or p.z < 1.22 or p.z > 1.475 or p.y < -0.015:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        ax = abs(p.x)
        front = smoothstep(-0.015, 0.025, p.y)
        # Round horizontal hemisphere centered around x~0.075
        u_in = smoothstep(0.006, 0.048, ax)
        u_out = 1.0 - smoothstep(0.100, 0.158, ax)
        if u_in <= 0.0 or u_out <= 0.0:
            return Vector()
        bx = (u_in ** 0.72) * (u_out ** 0.75)

        # True circular vertical dome: exponent 0.62 on upper arch makes Rows 2 (1.406) & 3 (1.362)
        # bow OUTWARD in a round convex curve instead of a flat straight slope!
        if p.z < 1.244 or p.z > 1.462:
            return Vector()
        if p.z < 1.332:
            dome_z = smoothstep(1.244, 1.332, p.z) ** 0.72
        elif p.z <= 1.368:
            dome_z = 1.0
        else:
            dome_z = (1.0 - smoothstep(1.368, 1.460, p.z)) ** 0.62

        # Fill the upper-chest hollow at Row 2 (z~1.406) gently so upper slope is round yet natural
        upper_round_fill = 1.0 + 0.11 * bell(p.z, 1.402, 0.030)

        # Gravity drop across the whole breast mound (strongest on lower pole z~1.296..1.335, plus natural
        # downward pull on z~1.406 so large breasts hang with real weight instead of a high push-up shelf!)
        grav_z = asym_bell(p.z, 1.246, 1.312, 1.442)

        # ~20% larger amplitude so max slider (2.0) achieves fuller, heavier volume
        dy = 0.056 * dome_z * upper_round_fill * bx * front
        dz = (-0.030 * grav_z - 0.008 * dome_z) * bx * front

        # Pin inframammary crease cleanly at Row 6 (z~1.242)
        crease_pin = bell(p.z, 1.242, 0.015) * bx * front
        dy -= 0.002 * crease_pin

        lateral_curve = sx * (0.017 * smoothstep(0.050, 0.118, ax) - 0.003 * bell(ax, 0.040, 0.022)) * dome_z * bx * front
        return Vector((lateral_curve, dy, dz)) * w

    make_key("Female_Breasts", fn_female_breasts, smin=0.0, smax=2.0, smooth_passes=1, smooth_strength=0.16)

    # Torso_V_Taper: Latissimus dorsi ("wings") & upper ribcage width
    def fn_v_taper(i, p, n):
        w = w_sum(i, "chest", "spine", "clavicle.L", "clavicle.R")
        if w < 0.02 or p.z < 1.08 or p.z > 1.50:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        vz = asym_bell(p.z, 1.08, 1.33, 1.48)
        lat_side = smoothstep(0.05, 0.145, abs(p.x))
        back_side = smoothstep(0.05, -0.08, p.y)
        return Vector((
            sx * 0.028 * vz * lat_side,
            -0.016 * vz * back_side * (0.4 + 0.6 * lat_side),
            0.0,
        )) * w

    make_key("Torso_V_Taper", fn_v_taper, smin=-0.60, smax=1.0)

    # Abdomen_Belly: Prominent belly / gut (positive) <-> flat/retracted stomach (negative)
    def fn_belly(i, p, n):
        w = w_sum(i, "spine", "hips", "chest")
        if w < 0.02 or p.z < 0.86 or p.z > 1.36:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        bz = asym_bell(p.z, 0.88, 1.055, 1.33)
        front = smoothstep(-0.05, 0.04, p.y)
        center_x = 1.0 - 0.35 * smoothstep(0.04, 0.16, abs(p.x))
        flank_z = asym_bell(p.z, 0.92, 1.04, 1.22)
        flank_x = smoothstep(0.06, 0.14, abs(p.x))
        sag = -0.016 * smoothstep(1.08, 0.93, p.z) * bz * front
        return Vector((
            sx * 0.032 * flank_z * flank_x,
            0.078 * bz * front * center_x,
            sag,
        )) * w

    make_key("Abdomen_Belly", fn_belly, smin=-0.55, smax=1.0, smooth_passes=3, smooth_strength=0.45)

    # Waist_Width: Lateral waist / obliques / midsection thickness
    def fn_waist(i, p, n):
        w = w_sum(i, "spine", "hips", "chest")
        if w < 0.02 or p.z < 0.94 or p.z > 1.28:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        wz = asym_bell(p.z, 0.95, 1.11, 1.27)
        lat = smoothstep(0.03, 0.13, abs(p.x))
        yn = 1.0 if p.y >= -0.025 else -1.0
        return Vector((
            sx * 0.034 * wz * lat,
            yn * 0.015 * wz,
            0.0,
        )) * w

    make_key("Waist_Width", fn_waist, smin=-0.65, smax=1.0)

    # Hips_Width: Pelvic / iliac crest / outer thigh sweep (long vertical span so shorts hem never shelves)
    def fn_hips(i, p, n):
        w = w_sum(i, "hips", "spine", "thigh.L", "thigh.R")
        if w < 0.02 or p.z < 0.52 or p.z > 1.16:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        hz = asym_bell(p.z, 0.53, 0.92, 1.15)
        lat = smoothstep(0.035, 0.135, abs(p.x))
        return Vector((sx * 0.022 * hz * lat, 0.004 * hz * lat, 0.0)) * w

    make_key("Hips_Width", fn_hips, smin=-0.60, smax=1.0, smooth_passes=3, smooth_strength=0.42)

    # Glutes_Mass: Silky-smooth round C-curve centered at z~0.885 (eliminates upper corner at z~0.95..0.99!)
    def fn_glutes(i, p, n):
        w = w_sum(i, "hips", "thigh.L", "thigh.R", "spine")
        if w < 0.02 or p.z < 0.58 or p.z > 1.12 or p.y > 0.005:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        ax = abs(p.x)
        back = smoothstep(-0.005, -0.065, p.y)
        if back <= 0.0:
            return Vector()
        # Hemispherical horizontal cheek dome (round from x=0.04 to x=0.12, preserving central cleft)
        cx_in = smoothstep(0.005, 0.045, ax)
        cx_out = 1.0 - smoothstep(0.102, 0.168, ax)
        cheek_x = 0.22 + 0.78 * ((cx_in ** 0.78) * (cx_out ** 0.78))

        # Center equator around z=0.865 and compensate Basis's sharp ridge at z~0.910!
        dome_z = asym_bell(p.z, 0.60, 0.865, 1.105) ** 0.80
        anti_basis_ridge = 1.0 - 0.30 * bell(p.z, 0.914, 0.036) + 0.14 * bell(p.z, 0.848, 0.038)
        shape = dome_z * anti_basis_ridge * back * cheek_x

        # Gentle downward weight on lower cheek (z~0.79); slight downward relax on z~0.91 so ridge rounds out
        lower_weight = bell(p.z, 0.805, 0.062) * back * cheek_x
        dz = -0.016 * lower_weight

        return Vector((
            sx * 0.013 * dome_z * smoothstep(0.05, 0.14, ax) * back,
            -0.068 * shape,
            dz,
        )) * w

    # Apply true 3D target-surface smoothing on Glutes_Mass so Basis's sharp vertex ring at z=0.910 rounds out!
    glute_raw = [fn_glutes(i, p, normals[i]) for i, p in enumerate(verts)]
    glute_target = [verts[i] + glute_raw[i] for i in range(len(verts))]
    for _ in range(4):
        nxt_target = []
        for i, p in enumerate(verts):
            sm_mask = (
                smoothstep(-0.02, -0.07, p.y)
                * asym_bell(p.z, 0.66, 0.89, 1.07)
                * smoothstep(0.022, 0.052, abs(p.x))
                * (1.0 - lock_weights[i])
            )
            if sm_mask <= 1e-4 or not adj[i]:
                nxt_target.append(glute_target[i])
                continue
            avg_p = Vector((0.0, 0.0, 0.0))
            for nb in adj[i]:
                avg_p += glute_target[nb]
            avg_p /= len(adj[i])
            nxt_target.append(glute_target[i].lerp(avg_p, 0.42 * sm_mask))
        glute_target = nxt_target
    glute_deltas = smooth_delta_field(
        [glute_target[i] - verts[i] for i in range(len(verts))],
        adj,
        lock_weights,
        passes=2,
        strength=0.30,
    )
    add_shape_key(obj, "Glutes_Mass", glute_deltas, slider_min=-0.60, slider_max=2.0)

    def fn_glutes_smoothed(i, p, n):
        return glute_deltas[i]

    # Arms_Upper: Biceps peak + Triceps horseshoe + Twist-safe cylindrical elbow transition
    def fn_upper_arms(i, p, n):
        w = w_sum(i, "upper_arm.L", "upper_arm.R", "clavicle.L", "clavicle.R", "forearm.L", "forearm.R")
        if w < 0.02:
            return Vector()
        seg_u = limb_info["upper_arm.R"][i] if p.x >= 0 else limb_info["upper_arm.L"][i]
        seg_f = limb_info["forearm.R"][i] if p.x >= 0 else limb_info["forearm.L"][i]
        t, _, rvec = seg_u
        tf, _, rvec_f = seg_f
        if t < -0.20 or tf > 0.30:
            return Vector()
        # Smoothly bridge radial vector across the elbow joint (t~1.0 / tf~0.0) so 90-deg forearm roll never pinches!
        elbow_blend = smoothstep(0.75, 1.05, t)
        r_combined = rvec.lerp(rvec_f, elbow_blend)
        if r_combined.length < 1e-5:
            return Vector()
        rn = r_combined.normalized()
        girth = asym_bell(t, -0.14, 0.44, 1.22) * (1.0 - smoothstep(0.05, 0.28, tf))
        # Directional bicep/tricep bellies fade out before the elbow joint so forearm twist stays round
        twist_safe = 1.0 - smoothstep(0.65, 0.92, t)
        bicep = asym_bell(t, 0.12, 0.48, 0.85) * max(0.0, rn.y * 0.75 + rn.z * 0.66) * twist_safe
        tricep = asym_bell(t, 0.06, 0.38, 0.80) * max(0.0, -rn.y * 0.80 - rn.z * 0.35) * twist_safe
        amp = 0.024 * girth + 0.019 * bicep + 0.017 * tricep
        return rn * (amp * w)

    make_key("Arms_Upper", fn_upper_arms, smin=-0.65, smax=1.0)

    # Forearms_Mass: Twist-safe cylindrical proximal forearm tapering cleanly to 0 at wrist
    def fn_forearms(i, p, n):
        w = w_sum(i, "forearm.L", "forearm.R", "upper_arm.L", "upper_arm.R")
        if w < 0.02:
            return Vector()
        seg_u = limb_info["upper_arm.R"][i] if p.x >= 0 else limb_info["upper_arm.L"][i]
        seg_f = limb_info["forearm.R"][i] if p.x >= 0 else limb_info["forearm.L"][i]
        tu, _, rvec_u = seg_u
        t, _, rvec = seg_f
        if tu < 0.72 or t > 0.90:
            return Vector()
        elbow_blend = smoothstep(0.72, 1.02, tu)
        r_combined = rvec_u.lerp(rvec, elbow_blend)
        if r_combined.length < 1e-5:
            return Vector()
        rn = r_combined.normalized()
        # Pure cylindrical radial expansion across elbow/forearm so twist in sword_idle stays 100% smooth
        belly_t = smoothstep(0.72, 1.0, tu) * asym_bell(t, -0.28, 0.26, 0.88)
        return rn * (0.025 * belly_t * w)

    make_key("Forearms_Mass", fn_forearms, smin=-0.65, smax=1.0)

    # Thighs_Mass: Quadriceps sweep + Hamstrings + outer sweep (inner groin protected)
    def fn_thighs(i, p, n):
        w = w_sum(i, "thigh.L", "thigh.R", "hips", "shin.L", "shin.R")
        if w < 0.02 or p.z < 0.44 or p.z > 0.99:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        seg = limb_info["thigh.R"][i] if p.x >= 0 else limb_info["thigh.L"][i]
        t, _, rvec = seg
        if t < -0.14 or t > 1.10 or rvec.length < 1e-5:
            return Vector()
        rn = rvec.normalized()
        outer = max(0.0, rn.x * sx)
        inner = max(0.0, -rn.x * sx)
        quad_front = max(0.0, rn.y)
        ham_back = max(0.0, -rn.y)
        profile = asym_bell(t, -0.10, 0.38, 1.05)
        quad_sweep = asym_bell(t, 0.06, 0.46, 0.94) * (0.65 * quad_front + 0.55 * outer)
        hamstring = asym_bell(t, 0.08, 0.42, 0.88) * ham_back
        amp = (0.029 * profile * (1.0 - 0.55 * inner) + 0.021 * quad_sweep + 0.018 * hamstring) * w
        return rn * amp

    make_key("Thighs_Mass", fn_thighs, smin=-0.65, smax=1.0)

    # Calves_Mass: High diamond gastrocnemius + soleus tapering to slim Achilles/ankle
    def fn_calves(i, p, n):
        w = w_sum(i, "shin.L", "shin.R", "thigh.L", "thigh.R")
        if w < 0.02 or p.z < 0.11 or p.z > 0.57:
            return Vector()
        seg = limb_info["shin.R"][i] if p.x >= 0 else limb_info["shin.L"][i]
        t, _, rvec = seg
        if t < -0.10 or t > 0.88 or rvec.length < 1e-5:
            return Vector()
        rn = rvec.normalized()
        back = max(0.0, -rn.y)
        side = abs(rn.x)
        front_shin = max(0.0, rn.y)
        gastroc = asym_bell(t, 0.02, 0.30, 0.74) * (0.88 * back + 0.48 * side)
        general = asym_bell(t, 0.02, 0.34, 0.84) * (1.0 - 0.65 * front_shin)
        return rn * ((0.034 * gastroc + 0.015 * general) * w)

    make_key("Calves_Mass", fn_calves, smin=-0.65, smax=1.0)

    # Legs_Full: Combined harmonious full-leg mass (Thighs + Knee transition + Calves)
    def fn_legs_full(i, p, n):
        d = fn_thighs(i, p, n) + fn_calves(i, p, n)
        # Smooth knee bridge around z ~ 0.518 so extreme full-leg values stay seamless
        w_kn = w_sum(i, "thigh.L", "thigh.R", "shin.L", "shin.R")
        if w_kn > 0.05 and 0.44 < p.z < 0.59:
            seg = limb_info["thigh.R"][i] if p.x >= 0 else limb_info["thigh.L"][i]
            _, _, rvec = seg
            if rvec.length > 1e-5:
                sx = 1.0 if p.x >= 0 else -1.0
                rn = rvec.normalized()
                inner = max(0.0, -rn.x * sx)
                d += rn * (0.010 * bell(p.z, 0.515, 0.07) * (1.0 - 0.5 * inner) * w_kn)
        return d

    make_key("Legs_Full", fn_legs_full, smin=-0.65, smax=1.0)

    # -------------------------------------------------------------------------
    # 2. HEAD & FACIAL STRUCTURE KEYS
    # -------------------------------------------------------------------------

    def fn_head_shape(i, p, n):
        w = vweights[i].get("head", 0.0)
        if w < 0.05 or p.z < 1.58:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        hz = smoothstep(1.60, 1.68, p.z)
        return Vector((sx * 0.013 * smoothstep(0.01, 0.07, abs(p.x)) * hz, 0.0, 0.006 * smoothstep(1.70, 1.80, p.z))) * w

    make_key("Head_Shape", fn_head_shape, smin=-0.65, smax=1.0)

    def fn_jaw_chin(i, p, n):
        w = w_sum(i, "head", "neck")
        if w < 0.05 or p.z < 1.54 or p.z > 1.70:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        jz = asym_bell(p.z, 1.55, 1.615, 1.685)
        front_jaw = smoothstep(-0.08, 0.02, p.y)
        chin = bell(p.z, 1.595, 0.04) * smoothstep(-0.01, 0.045, p.y) * (1.0 - smoothstep(0.02, 0.055, abs(p.x)))
        return Vector((
            sx * 0.014 * jz * front_jaw * smoothstep(0.015, 0.065, abs(p.x)),
            0.011 * chin,
            -0.008 * chin,
        )) * w

    make_key("Jaw_Chin", fn_jaw_chin, smin=-0.65, smax=1.0)

    def fn_cheeks_brow(i, p, n):
        w = vweights[i].get("head", 0.0)
        if w < 0.05 or p.z < 1.58:
            return Vector()
        sx = 1.0 if p.x >= 0 else -1.0
        cheek = bell(p.z, 1.648, 0.052) * asym_bell(abs(p.x), 0.015, 0.052, 0.088) * smoothstep(-0.06, 0.02, p.y)
        brow = bell(p.z, 1.718, 0.028) * smoothstep(-0.01, 0.04, p.y) * (1.0 - smoothstep(0.03, 0.075, abs(p.x)))
        return Vector((sx * 0.009 * cheek, 0.009 * cheek + 0.009 * brow, -0.002 * brow)) * w

    make_key("Face_Cheeks_Brow", fn_cheeks_brow, smin=-0.65, smax=1.0)

    # -------------------------------------------------------------------------
    # 3. HIGH-IMPACT MACRO ARCHETYPE KEYS (Feminine, Muscle, Heavy Fat, Gaunt, Elder)
    # -------------------------------------------------------------------------

    # Feminine_Body: Grounded, natural female proportions (smooth waist, subtle hip/thigh line, zero culote shelf)
    def fn_feminine_body(i, p, n):
        d = (
            fn_shoulders(i, p, n) * (-0.38)
            + fn_traps_neck(i, p, n) * (-0.36)
            + fn_v_taper(i, p, n) * (-0.36)
            + fn_waist(i, p, n) * (-0.26)
            + fn_hips(i, p, n) * 0.34
            + fn_glutes_smoothed(i, p, n) * 0.42
            + fn_thighs(i, p, n) * 0.10
            + fn_calves(i, p, n) * 0.08
            + fn_upper_arms(i, p, n) * (-0.24)
            + fn_forearms(i, p, n) * (-0.22)
            + fn_jaw_chin(i, p, n) * (-0.46)
        )
        sx = 1.0 if p.x >= 0 else -1.0
        w_torso = w_sum(i, "spine", "chest", "hips")
        # Gentle, elongated waist taper spanning 0.94..1.36 so transition into pelvis is completely gradual
        hourglass = asym_bell(p.z, 0.94, 1.14, 1.36) * smoothstep(0.03, 0.13, abs(p.x)) * w_torso
        d.x += sx * (-0.0045 * hourglass)
        d.y -= 0.003 * hourglass * (1.0 if p.y > 0 else -0.4)
        return d

    make_key("Feminine_Body", fn_feminine_body, smin=0.0, smax=1.0, smooth_passes=4, smooth_strength=0.45)

    # Body_Muscle: Full heroic muscular physique
    def fn_body_muscle(i, p, n):
        return (
            fn_shoulders(i, p, n) * 0.75
            + fn_traps_neck(i, p, n) * 0.70
            + fn_chest_pecs(i, p, n) * 0.85
            + fn_v_taper(i, p, n) * 0.75
            + fn_waist(i, p, n) * 0.18
            + fn_glutes(i, p, n) * 0.45
            + fn_upper_arms(i, p, n) * 0.90
            + fn_forearms(i, p, n) * 0.80
            + fn_thighs(i, p, n) * 0.80
            + fn_calves(i, p, n) * 0.80
            + fn_jaw_chin(i, p, n) * 0.30
        )

    make_key("Body_Muscle", fn_body_muscle, smin=0.0, smax=1.0)

    # Body_Heavy_Fat: Heavy / stout / tavern-keeper build
    def fn_body_fat(i, p, n):
        d = (
            fn_belly(i, p, n) * 0.95
            + fn_waist(i, p, n) * 0.85
            + fn_hips(i, p, n) * 0.65
            + fn_glutes(i, p, n) * 0.70
            + fn_thighs(i, p, n) * 0.72
            + fn_calves(i, p, n) * 0.48
            + fn_upper_arms(i, p, n) * 0.55
            + fn_forearms(i, p, n) * 0.38
            + fn_traps_neck(i, p, n) * 0.50
            + fn_cheeks_brow(i, p, n) * 0.55
        )
        # Soft lower chest / submental fullness (jowl/chin softness)
        w_ch = w_sum(i, "chest", "spine")
        if w_ch > 0.02 and 1.22 < p.z < 1.44 and p.y > -0.02:
            soft_pec = asym_bell(p.z, 1.23, 1.32, 1.43) * asym_bell(abs(p.x), 0.015, 0.085, 0.17)
            d += Vector((0.0, 0.026 * soft_pec, -0.010 * soft_pec)) * w_ch
        w_hd = w_sum(i, "head", "neck")
        if w_hd > 0.02 and 1.54 < p.z < 1.66:
            sx = 1.0 if p.x >= 0 else -1.0
            jowl = bell(p.z, 1.60, 0.05) * smoothstep(-0.06, 0.03, p.y)
            d += Vector((sx * 0.012 * smoothstep(0.01, 0.06, abs(p.x)) * jowl, 0.008 * jowl, -0.009 * jowl)) * w_hd
        return d

    make_key("Body_Heavy_Fat", fn_body_fat, smin=0.0, smax=1.0, smooth_passes=3, smooth_strength=0.46)

    # Body_Gaunt_Lean: Skinny / gaunt / wiry / underfed
    def fn_body_gaunt(i, p, n):
        return (
            fn_belly(i, p, n) * (-0.42)
            + fn_waist(i, p, n) * (-0.48)
            + fn_chest_pecs(i, p, n) * (-0.38)
            + fn_shoulders(i, p, n) * (-0.30)
            + fn_traps_neck(i, p, n) * (-0.45)
            + fn_glutes(i, p, n) * (-0.48)
            + fn_upper_arms(i, p, n) * (-0.52)
            + fn_forearms(i, p, n) * (-0.50)
            + fn_thighs(i, p, n) * (-0.52)
            + fn_calves(i, p, n) * (-0.48)
            + fn_cheeks_brow(i, p, n) * (-0.55)
        )

    make_key("Body_Gaunt_Lean", fn_body_gaunt, smin=0.0, smax=1.0)

    # Elder_Posture: Subtle aged stoop, upper thoracic rounding, hollowed cheeks & softened tone
    def fn_elder(i, p, n):
        d = (
            fn_traps_neck(i, p, n) * (-0.25)
            + fn_chest_pecs(i, p, n) * (-0.28)
            + fn_glutes(i, p, n) * (-0.35)
            + fn_upper_arms(i, p, n) * (-0.28)
            + fn_cheeks_brow(i, p, n) * (-0.50)
        )
        # Gentle thoracic kyphosis + forward neck/head carriage (kept within bone capsules)
        w_up = w_sum(i, "chest", "neck", "head")
        if w_up > 0.02 and p.z > 1.25:
            stoop = smoothstep(1.26, 1.62, p.z)
            kyph = bell(p.z, 1.44, 0.16) * smoothstep(0.02, -0.10, p.y)
            d += Vector((0.0, 0.024 * stoop - 0.015 * kyph, -0.014 * stoop)) * w_up
        return d

    make_key("Elder_Posture", fn_elder, smin=0.0, smax=1.0)


def ensure_female_placeholder_texture(obj):
    """Create Chartest-Female-v01-128.png placeholder if missing, preserving exact 128x128 UV layout.

    Never overwrites an existing Chartest-Female-v01-128.png so user hand-painted edits stay safe.
    """
    male_png = CHARTEST_DIR / "Chartest-Grimora-v02-128.png"
    female_png = CHARTEST_DIR / "Chartest-Female-v01-128.png"
    if female_png.exists():
        return female_png

    src_img = bpy.data.images.load(str(male_png), check_existing=True)
    w, h = src_img.size
    px = list(src_img.pixels[:])

    def get_rgba(x, y):
        x = max(0, min(w - 1, x))
        y = max(0, min(h - 1, y))
        idx = (y * w + x) * 4
        return px[idx : idx + 4]

    def set_rgba(x, y, rgba):
        if 0 <= x < w and 0 <= y < h:
            idx = (y * w + x) * 4
            px[idx : idx + 4] = rgba

    # Sample the authentic dark cloth palette from the shorts island so the placeholder bandeau
    # matches the exact PSX cloth grain and contrast of Chartest-Grimora-v02-128.png.
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    uv_layer = bm.loops.layers.uv.active

    cloth_samples = []
    for f in bm.faces:
        cz = sum(v.co.z for v in f.verts) / len(f.verts)
        if 0.78 < cz < 0.88:
            uv_c = sum((l[uv_layer].uv for l in f.loops), Vector((0.0, 0.0))) / len(f.loops)
            cloth_samples.append(get_rgba(int(uv_c.x * w), int(uv_c.y * h)))
    if not cloth_samples:
        cloth_samples = [(0.20, 0.18, 0.16, 1.0)]

    # Slightly warm/soften skin midtones by ~4% so texture swap is perceptible even on limbs/face
    for y in range(h):
        for x in range(w):
            r, g, b, a = get_rgba(x, y)
            if r > b * 1.22 and r > 0.18:  # skin pixel heuristic
                set_rgba(x, y, (min(1.0, r * 1.045), min(1.0, g * 1.035), min(1.0, b * 1.025), a))

    # Rasterize a clean horizontal cloth wrap/bandeau across chest faces (z in [1.295, 1.415])
    def point_in_tri(pt, v0, v1, v2):
        d1 = (pt.x - v1.x) * (v0.y - v1.y) - (v0.x - v1.x) * (pt.y - v1.y)
        d2 = (pt.x - v2.x) * (v1.y - v2.y) - (v1.x - v2.x) * (pt.y - v2.y)
        d3 = (pt.x - v0.x) * (v2.y - v0.y) - (v2.x - v0.x) * (pt.y - v0.y)
        has_neg = (d1 < -1e-5) or (d2 < -1e-5) or (d3 < -1e-5)
        has_pos = (d1 > 1e-5) or (d2 > 1e-5) or (d3 > 1e-5)
        return not (has_neg and has_pos)

    deform = bm.verts.layers.deform.verify()
    arm_groups = {
        g.index
        for g in obj.vertex_groups
        if ("arm" in g.name.lower() or "hand" in g.name.lower())
    }

    for f in bm.faces:
        zs = [v.co.z for v in f.verts]
        if max(zs) < 1.235 or min(zs) > 1.425:
            continue
        # Skip faces that belong predominantly to the arms
        if all(sum(v[deform].get(gi, 0.0) for gi in arm_groups) > 0.45 for v in f.verts):
            continue
        loops = list(f.loops)
        for k in range(1, len(loops) - 1):
            tri_loops = (loops[0], loops[k], loops[k + 1])
            uvs = [Vector((l[uv_layer].uv.x * w, l[uv_layer].uv.y * h)) for l in tri_loops]
            p3d = [l.vert.co for l in tri_loops]
            v_arm = [sum(l.vert[deform].get(gi, 0.0) for gi in arm_groups) for l in tri_loops]
            min_x = max(0, int(math.floor(min(u.x for u in uvs))))
            max_x = min(w - 1, int(math.ceil(max(u.x for u in uvs))))
            min_y = max(0, int(math.floor(min(u.y for u in uvs))))
            max_y = min(h - 1, int(math.ceil(max(u.y for u in uvs))))
            denom = (uvs[1].y - uvs[2].y) * (uvs[0].x - uvs[2].x) + (uvs[2].x - uvs[1].x) * (uvs[0].y - uvs[2].y)
            if abs(denom) < 1e-6:
                continue
            for py in range(min_y, max_y + 1):
                for px_i in range(min_x, max_x + 1):
                    pt = Vector((px_i + 0.5, py + 0.5))
                    if not point_in_tri(pt, uvs[0], uvs[1], uvs[2]):
                        continue
                    w0 = ((uvs[1].y - uvs[2].y) * (pt.x - uvs[2].x) + (uvs[2].x - uvs[1].x) * (pt.y - uvs[2].y)) / denom
                    w1 = ((uvs[2].y - uvs[0].y) * (pt.x - uvs[2].x) + (uvs[0].x - uvs[2].x) * (pt.y - uvs[2].y)) / denom
                    w2 = 1.0 - w0 - w1
                    arm_at = w0 * v_arm[0] + w1 * v_arm[1] + w2 * v_arm[2]
                    x_at = w0 * p3d[0].x + w1 * p3d[1].x + w2 * p3d[2].x
                    if arm_at > 0.34 or abs(x_at) > 0.176:
                        continue
                    z_at = w0 * p3d[0].z + w1 * p3d[1].z + w2 * p3d[2].z
                    y_at = w0 * p3d[0].y + w1 * p3d[1].y + w2 * p3d[2].y
                    # Bandeau top: covers full front breast down to inframammary fold (1.250..1.404), narrower back strap (1.300..1.380)
                    fb = smoothstep(-0.045, 0.020, y_at)
                    z_lo = 1.300 + (1.250 - 1.300) * fb
                    z_hi = 1.380 + (1.404 - 1.380) * fb
                    if z_lo <= z_at <= z_hi:
                        old_c = get_rgba(px_i, py)
                        lum = 0.299 * old_c[0] + 0.587 * old_c[1] + 0.114 * old_c[2]
                        sample = cloth_samples[(px_i * 7 + py * 13) % len(cloth_samples)]
                        # Preserve underlying pectoral shading + subtle PSX bust volume modulation!
                        shade = 0.72 + 0.65 * lum
                        if y_at >= 0.0:
                            dome = bell(abs(x_at), 0.068, 0.058) * bell(z_at, 1.345, 0.050)
                            under = bell(abs(x_at), 0.068, 0.065) * smoothstep(1.290, 1.250, z_at)
                            valley = smoothstep(0.026, 0.004, abs(x_at)) * 0.14
                            shade *= 1.0 + 0.22 * dome - 0.24 * under - valley
                        hem = 0.82 if (abs(z_at - z_lo) < 0.010 or abs(z_at - z_hi) < 0.010) else 1.0
                        set_rgba(
                            px_i,
                            py,
                            (
                                min(1.0, sample[0] * shade * hem),
                                min(1.0, sample[1] * shade * hem),
                                min(1.0, sample[2] * shade * hem),
                                1.0,
                            ),
                        )

    bm.free()
    out_img = bpy.data.images.new("Chartest-Female-v01-128.png", width=w, height=h, alpha=True)
    out_img.pixels[:] = px
    out_img.filepath_raw = str(female_png)
    out_img.file_format = "PNG"
    out_img.save()
    bpy.data.images.remove(out_img)
    print(f"[sex-param] created placeholder female texture -> {female_png.name}")
    return female_png


def author_sex_parameter(obj, arm):
    """Add `is_female`, `breasts_size`, and `glutes_size` on chartest_rig, driving silhouette + texture."""
    male_png = CHARTEST_DIR / "Chartest-Grimora-v02-128.png"
    female_png = ensure_female_placeholder_texture(obj)

    arm["is_female"] = False
    ui_fem = arm.id_properties_ui("is_female")
    ui_fem.update(
        default=False,
        description="Toggle Female base silhouette (Feminine_Body + Female_Breasts) and Female texture slot",
    )

    arm["breasts_size"] = 1.00
    ui_bust = arm.id_properties_ui("breasts_size")
    ui_bust.update(
        min=0.0,
        max=2.00,
        soft_min=0.0,
        soft_max=2.00,
        default=1.00,
        precision=2,
        description="Female breast size (0.0 = flat, 0.5 = small, 1.0 = medium default, 1.5 = large, 2.0 = max)",
    )

    arm["glutes_size"] = 0.00
    ui_glutes = arm.id_properties_ui("glutes_size")
    ui_glutes.update(
        min=-0.60,
        max=2.00,
        soft_min=-0.60,
        soft_max=2.00,
        default=0.00,
        precision=2,
        description="Glutes volume slider (-0.60 = flat, 0.0 = default for sex, 1.0 = full, 2.0 = max)",
    )

    obj.data.shape_keys.animation_data_create()

    # 1. Drive Feminine_Body from is_female
    fc_body = obj.data.shape_keys.driver_add('key_blocks["Feminine_Body"].value')
    drv_body = fc_body.driver
    drv_body.type = "SCRIPTED"
    v_fem = drv_body.variables.new()
    v_fem.name = "fem"
    v_fem.type = "SINGLE_PROP"
    v_fem.targets[0].id_type = "OBJECT"
    v_fem.targets[0].id = arm
    v_fem.targets[0].data_path = '["is_female"]'
    drv_body.expression = "min(1.0, max(0.0, fem))"
    assert drv_body.is_simple_expression

    # 2. Drive Female_Breasts from is_female * breasts_size (0.0 .. 2.0)
    fc_bust = obj.data.shape_keys.driver_add('key_blocks["Female_Breasts"].value')
    drv_bust = fc_bust.driver
    drv_bust.type = "SCRIPTED"
    vf = drv_bust.variables.new()
    vf.name = "fem"
    vf.type = "SINGLE_PROP"
    vf.targets[0].id_type = "OBJECT"
    vf.targets[0].id = arm
    vf.targets[0].data_path = '["is_female"]'
    vb = drv_bust.variables.new()
    vb.name = "bust"
    vb.type = "SINGLE_PROP"
    vb.targets[0].id_type = "OBJECT"
    vb.targets[0].id = arm
    vb.targets[0].data_path = '["breasts_size"]'
    drv_bust.expression = "min(2.0, max(0.0, fem * bust))"
    assert drv_bust.is_simple_expression

    # 3. Drive Glutes_Mass from glutes_size (-0.60 .. 2.0) — works for both male & female!
    fc_glutes = obj.data.shape_keys.driver_add('key_blocks["Glutes_Mass"].value')
    drv_glutes = fc_glutes.driver
    drv_glutes.type = "SCRIPTED"
    vg = drv_glutes.variables.new()
    vg.name = "glutes"
    vg.type = "SINGLE_PROP"
    vg.targets[0].id_type = "OBJECT"
    vg.targets[0].id = arm
    vg.targets[0].data_path = '["glutes_size"]'
    drv_glutes.expression = "min(2.0, max(-0.6, glutes))"
    assert drv_glutes.is_simple_expression

    # 3. Configure M_Char material to switch between Male & Female 128x128 textures via `is_female`!
    mat = obj.data.materials[0] if obj.data.materials else bpy.data.materials.new("M_Char")
    if not obj.data.materials:
        obj.data.materials.append(mat)
    mat.use_nodes = True
    nt = mat.node_tree
    nt.animation_data_clear()
    nodes = nt.nodes
    links = nt.links
    nodes.clear()

    img_male = bpy.data.images.load(str(male_png), check_existing=True)
    img_female = bpy.data.images.load(str(female_png), check_existing=True)

    tex_m = nodes.new("ShaderNodeTexImage")
    tex_m.name = "Tex_Male"
    tex_m.label = "Male Texture (128x128)"
    tex_m.location = (-520, 180)
    tex_m.image = img_male
    tex_m.interpolation = "Closest"

    tex_f = nodes.new("ShaderNodeTexImage")
    tex_f.name = "Tex_Female"
    tex_f.label = "Female Texture (128x128)"
    tex_f.location = (-520, -140)
    tex_f.image = img_female
    tex_f.interpolation = "Closest"

    mix = nodes.new("ShaderNodeMix")
    mix.name = "Sex_Tex_Switch"
    mix.label = "is_female Texture Switch"
    mix.location = (-200, 60)
    mix.data_type = "RGBA"
    mix.clamp_factor = True
    mix.inputs[0].default_value = 0.0

    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.location = (60, 60)
    bsdf.inputs["Roughness"].default_value = 0.86
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.08

    out = nodes.new("ShaderNodeOutputMaterial")
    out.location = (340, 60)

    links.new(tex_m.outputs["Color"], mix.inputs[6])
    links.new(tex_f.outputs["Color"], mix.inputs[7])
    links.new(mix.outputs[2], bsdf.inputs["Base Color"])
    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])

    fc_mat = nt.driver_add('nodes["Sex_Tex_Switch"].inputs[0].default_value')
    drv_mat = fc_mat.driver
    drv_mat.type = "SCRIPTED"
    vm = drv_mat.variables.new()
    vm.name = "fem"
    vm.type = "SINGLE_PROP"
    vm.targets[0].id_type = "OBJECT"
    vm.targets[0].id = arm
    vm.targets[0].data_path = '["is_female"]'
    drv_mat.expression = "min(1.0, max(0.0, fem))"
    assert drv_mat.is_simple_expression

    return {
        "property": "is_female",
        "type": "bool",
        "default": False,
        "breasts_property": "breasts_size",
        "breasts_range": [0.0, 2.0],
        "breasts_default": 1.0,
        "glutes_property": "glutes_size",
        "glutes_range": [-0.6, 2.0],
        "glutes_default": 0.0,
        "male": {
            "material_index": 0,
            "material_name": "M_Char",
            "texture": male_png.name,
            "morph_weights": {},
        },
        "female": {
            "material_index": 1,
            "material_name": "M_Char_Female",
            "texture": female_png.name,
            "morph_weights": {"Feminine_Body": 1.0, "Female_Breasts": 1.0},
        },
    }


def author_proportional_height_system(obj, arm):
    """Author true allometric Height controller (`height_m`: 0.40m .. 2.60m, default 1.82m).

    Instead of naive Y-stretching or pinhead uniform scaling:
    - Short stature (down to 0.40m): proportionally shorter legs & forearms relative to torso,
      larger head-to-body canon (~4.5 heads at 0.40m vs 7.6 heads at 1.82m), sturdier square-cube build.
    - Tall stature (up to 2.60m): proportionally longer legs & reach, broader heroic upper frame,
      smaller head-to-height ratio (~8.9 heads at 2.60m).
    - Exact LBS co-deformation shape keys (`_Height_Short_Comp`, `_Height_Tall_Comp`) cancel 100%
      of intra-segment skinning residuals, keeping feet planted at Z=0.000m, crown at exact `height_m`,
      and zero non-uniform bone scale shear in animations!
    """
    basis = obj.data.shape_keys.key_blocks[0]
    verts = [basis.data[i].co.copy() for i in range(len(obj.data.vertices))]
    vweights = get_group_weights(obj)
    adj = build_vertex_adjacency(obj.data)

    z_min = min(p.z for p in verts)
    z_max = max(p.z for p in verts)
    h0 = round(z_max - max(0.0, z_min), 5)  # ~1.81725 m

    bones = {b.name: b for b in arm.data.bones}
    z_hips = bones["hips"].head_local.z       # 0.8905
    z_knee = bones["shin.L"].head_local.z     # 0.5179
    z_ankle = bones["foot.L"].head_local.z    # 0.0999
    z_spine = bones["spine"].head_local.z     # 1.0696
    z_chest = bones["chest"].head_local.z     # 1.3021
    z_neck = bones["neck"].head_local.z       # 1.5288
    z_head = bones["head"].head_local.z       # 1.6025
    head_pivot = bones["head"].head_local.copy()

    # World-space joint shifts (pre-root-scale) for Short (u_s=1 at 0.40m) and Tall (u_t=1 at 2.60m)
    # Note: Δz_hips + Δz_shin_rel + Δz_foot_rel == 0.0000 EXACTLY so ankles/soles never leave Z=0!
    short_spec = {
        "hips_dz": -0.180,
        "shin_dz_rel": +0.092,
        "foot_dz_rel": +0.088,   # -0.180 + 0.092 + 0.088 == 0.000!
        "spine_dz_rel": -0.010,
        "chest_dz_rel": -0.014,
        "neck_dz_rel": -0.014,
        "head_dz_rel": -0.006,
        "arm_shorten": 0.16,     # 16% shorter upper_arm & forearm along their bone axes
        "head_scale_add": +0.46, # 1.46x head relative to body -> ~4.5-head canon at 0.40m!
        "girth_boost": +0.22,    # Square-cube law sturdier build
    }
    tall_spec = {
        "hips_dz": +0.104,
        "shin_dz_rel": -0.054,
        "foot_dz_rel": -0.050,   # +0.104 - 0.054 - 0.050 == 0.000!
        "spine_dz_rel": +0.014,
        "chest_dz_rel": +0.018,
        "neck_dz_rel": +0.012,
        "head_dz_rel": +0.004,
        "arm_shorten": -0.09,    # 9% longer upper_arm & forearm reach
        "head_scale_add": -0.14, # 0.86x head relative to body -> ~8.9-head giant canon at 2.60m!
        "girth_boost": +0.06,    # Slight heroic frame solidity
    }

    def build_joint_world_shifts(spec):
        """Return world-space translation shift for every bone in bind pose (before root scale)."""
        rel = defaultdict( lambda: Vector((0.0, 0.0, 0.0)) )
        rel["hips"] = Vector((0.0, 0.0, spec["hips_dz"]))
        for side in ("L", "R"):
            # Shift knee & ankle along exact leg vectors while keeping exact Z cancellation
            thigh_vec = bones[f"shin.{side}"].head_local - bones[f"thigh.{side}"].head_local
            shin_vec = bones[f"foot.{side}"].head_local - bones[f"shin.{side}"].head_local
            s_thigh = spec["shin_dz_rel"] / thigh_vec.z  # thigh_vec.z is negative (~ -0.405)
            s_shin = spec["foot_dz_rel"] / shin_vec.z    # shin_vec.z is negative (~ -0.418)
            # Keep X/Y proportional along limb axis, cancel residual at foot so foot stays grounded
            d_shin = Vector((thigh_vec.x * s_thigh, thigh_vec.y * s_thigh, spec["shin_dz_rel"]))
            d_foot = Vector((-d_shin.x, -d_shin.y, spec["foot_dz_rel"]))
            rel[f"shin.{side}"] = d_shin
            rel[f"foot.{side}"] = d_foot

            # Shorten/lengthen upper_arm and forearm along their exact bone segments
            ua_vec = bones[f"forearm.{side}"].head_local - bones[f"upper_arm.{side}"].head_local
            fa_vec = bones[f"hand.{side}"].head_local - bones[f"forearm.{side}"].head_local
            rel[f"forearm.{side}"] = -ua_vec * spec["arm_shorten"]
            rel[f"hand.{side}"] = -fa_vec * spec["arm_shorten"]

        rel["spine"] = Vector((0.0, 0.0, spec["spine_dz_rel"]))
        rel["chest"] = Vector((0.0, 0.0, spec["chest_dz_rel"]))
        rel["neck"] = Vector((0.0, 0.0, spec["neck_dz_rel"]))
        rel["head"] = Vector((0.0, 0.0, spec["head_dz_rel"]))

        # Accumulate global shifts down the hierarchy
        world_shift = {}
        def accum(b):
            if b.name in world_shift:
                return world_shift[b.name]
            p_shift = accum(b.parent) if b.parent else Vector((0.0, 0.0, 0.0))
            world_shift[b.name] = p_shift + rel[b.name]
            return world_shift[b.name]

        for b in arm.data.bones:
            accum(b)
        return rel, world_shift

    short_rel, short_world = build_joint_world_shifts(short_spec)
    tall_rel, tall_world = build_joint_world_shifts(tall_spec)

    def build_allometry_comp_deltas(spec, world_shift):
        """Compute exact residual `v_ideal - v_rest - Δv_rig` so LBS + ShapeKey = smooth anatomy."""
        # 1. Compute Δv_rig produced by pose bone translations + uniform head scale
        rig_deltas = []
        new_head_pivot = head_pivot + world_shift["head"]
        for i, p in enumerate(verts):
            dv = Vector((0.0, 0.0, 0.0))
            total_w = sum(vweights[i].get(b.name, 0.0) for b in arm.data.bones)
            inv_w = 1.0 / total_w if total_w > 1e-6 else 0.0
            for b in arm.data.bones:
                wb = vweights[i].get(b.name, 0.0) * inv_w
                if wb <= 0.0:
                    continue
                if b.name == "head":
                    # Head bone applies world_shift['head'] AND uniform scale (1 + head_scale_add) about head_pivot
                    dv += wb * (world_shift["head"] + (p - head_pivot) * spec["head_scale_add"])
                else:
                    dv += wb * world_shift[b.name]
            rig_deltas.append(dv)

        # 2. Compute smooth ideal anatomical target position for every vertex
        #    Piecewise vertical map matching exact new joint Z heights:
        #    0 -> z_ankle (unchanged!), z_ankle -> z_knee -> z_hips -> z_spine -> z_chest -> z_neck -> z_head
        z_stops_old = [0.0, z_ankle, z_knee, z_hips, z_spine, z_chest, z_neck, z_head]
        z_stops_new = [
            0.0,
            z_ankle + world_shift["foot.L"].z,
            z_knee + world_shift["shin.L"].z,
            z_hips + world_shift["hips"].z,
            z_spine + world_shift["spine"].z,
            z_chest + world_shift["chest"].z,
            z_neck + world_shift["neck"].z,
            z_head + world_shift["head"].z,
        ]

        def piecewise_z(z):
            if z <= z_stops_old[0]:
                return z
            for k in range(len(z_stops_old) - 1):
                a0, a1 = z_stops_old[k], z_stops_old[k + 1]
                b0, b1 = z_stops_new[k], z_stops_new[k + 1]
                if z <= a1:
                    t = (z - a0) / max(a1 - a0, 1e-6)
                    return b0 + t * (b1 - b0)
            return z_stops_new[-1] + (z - z_stops_old[-1]) * (1.0 + spec["head_scale_add"])

        comp_raw = []
        for i, p in enumerate(verts):
            w_hd = vweights[i].get("head", 0.0)
            w_arm_l = sum(vweights[i].get(k, 0.0) for k in ("upper_arm.L", "forearm.L", "hand.L", "thumb.L", "thumb_02_l_013", "thumb_03_l_014", "index_01_l_015", "index_02_l_016", "index_03_l_017", "fingers.L", "fingertips.L", "fingers_03_l_020"))
            w_arm_r = sum(vweights[i].get(k, 0.0) for k in ("upper_arm.R", "forearm.R", "hand.R", "thumb.R", "thumb_02_r_028", "thumb_03_r_029", "index_01_r_030", "index_02_r_031", "index_03_r_032", "fingers.R", "fingertips.R", "fingers_03_r_035"))
            w_hand = sum(vweights[i].get(k, 0.0) for k in vweights[i] if any(h in k.lower() for h in ("hand", "thumb", "index", "finger")))

            # For hands & head, rig transform is ALREADY exact rigid/smooth — keep residual = 0 on hands!
            if w_hand > 0.5:
                comp_raw.append(Vector((0.0, 0.0, 0.0)))
                continue

            # Body & leg smooth piecewise Z + arm axial interpolation
            ideal_body = Vector((p.x, p.y, piecewise_z(p.z)))

            # Add subtle allometric girth (stockier short folk / broader tall colossus) on torso & limbs
            g = spec["girth_boost"]
            if 0.14 < p.z < 1.54 and abs(g) > 1e-5:
                g_mask = smoothstep(0.14, 0.28, p.z) * (1.0 - smoothstep(1.44, 1.54, p.z)) * (1.0 - max(w_arm_l, w_arm_r))
                sx = 1.0 if p.x >= 0 else -1.0
                # Leg radial vs torso radial
                if p.z < 0.88:
                    leg_cx = sx * (0.145 if p.z < 0.55 else 0.115)
                    ideal_body.x += (p.x - leg_cx) * g * 0.85 * g_mask
                    ideal_body.y += (p.y + 0.03) * g * 0.85 * g_mask
                else:
                    ideal_body.x += p.x * g * 0.75 * g_mask
                    ideal_body.y += (p.y + 0.04) * g * 0.65 * g_mask

            # Blend cleanly to pure rig delta on arms and head so joints stay 100% seamless
            w_rig_exact = min(1.0, w_hd + max(w_arm_l, w_arm_r))
            target_delta = (ideal_body - p) * (1.0 - w_rig_exact) + rig_deltas[i] * w_rig_exact
            residual = target_delta - rig_deltas[i]
            if p.z < 0.04:
                residual *= smoothstep(0.01, 0.04, p.z)
            comp_raw.append(residual)

        # Gentle smooth on residual while keeping hands/soles at 0
        lock = [1.0 if p.z < 0.015 else min(1.0, sum(vweights[i].get(k, 0.0) for k in vweights[i] if any(h in k.lower() for h in ("hand", "thumb", "index", "finger", "head")))) for i, p in enumerate(verts)]
        comp = smooth_delta_field(comp_raw, adj, lock, passes=2, strength=0.35)
        return comp, rig_deltas

    short_comp, short_rig_deltas = build_allometry_comp_deltas(short_spec, short_world)
    tall_comp, tall_rig_deltas = build_allometry_comp_deltas(tall_spec, tall_world)

    sk_short = add_shape_key(obj, "_Height_Short_Comp", short_comp, slider_min=0.0, slider_max=1.0)
    sk_tall = add_shape_key(obj, "_Height_Tall_Comp", tall_comp, slider_min=0.0, slider_max=1.0)

    # Compute exact unscaled crown heights at u_s=1 (0.40m) and u_t=1 (2.60m)
    c_short = max((verts[i] + short_rig_deltas[i] + short_comp[i]).z for i in range(len(verts))) - max(0.0, z_min)
    c_tall = max((verts[i] + tall_rig_deltas[i] + tall_comp[i]).z for i in range(len(verts))) - max(0.0, z_min)
    dc_short = round(c_short - h0, 6)
    dc_tall = round(c_tall - h0, 6)
    d_s = round(h0 - 0.40, 6)
    d_t = round(2.60 - h0, 6)

    # Configure `height_m` custom property on chartest_rig (0.40m .. 2.60m)
    arm["height_m"] = round(h0, 2)
    ui = arm.id_properties_ui("height_m")
    ui.update(
        min=0.40,
        max=2.60,
        soft_min=0.40,
        soft_max=2.60,
        default=round(h0, 2),
        step=1.0,
        precision=2,
        subtype="DISTANCE",
        description="Character height in meters (0.40m - 2.60m) with automatic allometric proportions",
    )

    arm.animation_data_create()

    def bind_driver(id_block, data_path, index, expr):
        fc = id_block.driver_add(data_path, index) if index is not None else id_block.driver_add(data_path)
        drv = fc.driver
        drv.type = "SCRIPTED"
        var = drv.variables.new()
        var.name = "h"
        var.type = "SINGLE_PROP"
        var.targets[0].id_type = "OBJECT"
        var.targets[0].id = arm
        var.targets[0].data_path = '["height_m"]'
        drv.expression = expr
        assert drv.is_simple_expression, f"Non-simple driver expression: {expr}"
        return fc

    expr_us = f"max(0.0, min(1.0, ({h0} - h) / {d_s}))"
    expr_ut = f"max(0.0, min(1.0, (h - {h0}) / {d_t}))"
    # Clamp h safely to [0.25, 3.5] inside root scale so extreme manual typing stays well-behaved
    expr_h_safe = "max(0.25, min(3.5, h))"
    expr_root_scale = f"{expr_h_safe} / ({h0} + {expr_us} * ({dc_short}) + {expr_ut} * ({dc_tall}))"

    # 1. Root uniform scale driver
    for ax in range(3):
        bind_driver(arm, 'pose.bones["root"].scale', ax, expr_root_scale)

    # 2. Head uniform allometric scale driver
    expr_head_scale = f"1.0 + {expr_us} * ({short_spec['head_scale_add']}) + {expr_ut} * ({tall_spec['head_scale_add']})"
    for ax in range(3):
        bind_driver(arm, 'pose.bones["head"].scale', ax, expr_head_scale)

    # 3. Pose-bone local translation drivers (converting world Δ into each bone's rest local frame!)
    bone_local_recipe = {}
    all_shifted_bones = sorted(set(short_rel.keys()) | set(tall_rel.keys()))
    for bname in all_shifted_bones:
        r_inv = bones[bname].matrix_local.to_3x3().transposed()
        loc_s = r_inv @ short_rel[bname]
        loc_t = r_inv @ tall_rel[bname]
        if loc_s.length < 1e-6 and loc_t.length < 1e-6:
            continue
        bone_local_recipe[bname] = {
            "short_local_m": [round(x, 6) for x in loc_s],
            "tall_local_m": [round(x, 6) for x in loc_t],
            "short_world_m": [round(x, 6) for x in short_rel[bname]],
            "tall_world_m": [round(x, 6) for x in tall_rel[bname]],
        }
        for ax in range(3):
            cs = round(loc_s[ax], 6)
            ct = round(loc_t[ax], 6)
            if abs(cs) < 1e-6 and abs(ct) < 1e-6:
                continue
            expr_loc = f"{expr_us} * ({cs}) + {expr_ut} * ({ct})"
            bind_driver(arm, f'pose.bones["{bname}"].location', ax, expr_loc)

    # 4. Shape key drivers for `_Height_Short_Comp` & `_Height_Tall_Comp`
    obj.data.shape_keys.animation_data_create()
    bind_driver(obj.data.shape_keys, 'key_blocks["_Height_Short_Comp"].value', None, expr_us)
    bind_driver(obj.data.shape_keys, 'key_blocks["_Height_Tall_Comp"].value', None, expr_ut)

    return {
        "property": "height_m",
        "range_m": [0.40, 2.60],
        "default_m": round(h0, 4),
        "unscaled_crown_short_m": round(c_short, 5),
        "unscaled_crown_tall_m": round(c_tall, 5),
        "head_scale_short": 1.0 + short_spec["head_scale_add"],
        "head_scale_tall": 1.0 + tall_spec["head_scale_add"],
        "morph_short_comp": "_Height_Short_Comp",
        "morph_tall_comp": "_Height_Tall_Comp",
        "joint_shifts": bone_local_recipe,
    }


def validate_height_and_shapes(obj, arm):
    """Verify exact evaluated world-space height across [0.40m .. 2.60m]."""
    for t in arm.animation_data.nla_tracks:
        t.mute = True
    arm.animation_data.action = None

    depsgraph = bpy.context.evaluated_depsgraph_get()
    results = []
    for target_h in (0.40, 0.85, 1.35, 1.82, 2.20, 2.60):
        arm["height_m"] = float(target_h)
        arm.update_tag()
        obj.update_tag()
        depsgraph.update()
        eval_obj = obj.evaluated_get(depsgraph)
        eval_mesh = eval_obj.to_mesh()
        zs = [(eval_obj.matrix_world @ v.co).z for v in eval_mesh.vertices]
        xs = [(eval_obj.matrix_world @ v.co).x for v in eval_mesh.vertices]
        z0, z1 = min(zs), max(zs)
        width = max(xs) - min(xs)
        eval_obj.to_mesh_clear()
        measured = z1 - max(0.0, z0)
        err_mm = abs(measured - target_h) * 1000.0
        print(f"[height-check] target={target_h:.2f}m -> min_z={z0:+.4f}m, max_z={z1:.4f}m, span={width:.3f}m (err={err_mm:.2f}mm)")
        assert err_mm < 2.5, f"Height error {err_mm:.2f}mm exceeds 2.5mm at {target_h}m"
        assert abs(z0) < 0.003, f"Foot sole left ground ({z0:+.4f}m) at {target_h}m"
        results.append({"target_m": target_h, "measured_m": round(measured, 4), "min_z_m": round(z0, 4)})

    arm["height_m"] = 1.82
    # Keep NLA tracks muted in saved .blend so Blender opens in clean symmetrical A-pose
    # (preventing 9 stacked NLA tracks from candy-wrapping forearm.R while modeling hair/clothes!)
    for pb in arm.pose.bones:
        pb.rotation_mode = "QUATERNION"
        pb.rotation_quaternion = (1.0, 0.0, 0.0, 0.0)
    depsgraph.update()
    return results


def transplant_with_morphs_to_gltf(obj, arm, height_meta, sex_meta, stage_dir):
    """Export skinned mesh + all Morph Targets + Female variant cleanly onto chartest.gltf."""
    stage_dir.mkdir(parents=True, exist_ok=True)
    temp_gltf = stage_dir / "temp_morphs.gltf"

    # Reset height_m & is_female to rest baseline so exported Basis is pure Male bind space
    rest_h = height_meta["default_m"]
    arm["height_m"] = rest_h
    arm["is_female"] = False
    for t in arm.animation_data.nla_tracks:
        t.mute = True
    arm.animation_data.action = None
    bpy.context.evaluated_depsgraph_get().update()

    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    arm.select_set(True)
    bpy.context.view_layer.objects.active = arm

    bpy.ops.export_scene.gltf(
        filepath=str(temp_gltf),
        export_format="GLTF_SEPARATE",
        use_selection=True,
        export_yup=True,
        export_animations=False,
        export_skins=True,
        export_def_bones=False,
        export_morph=True,
        export_morph_normal=False,
        export_morph_tangent=False,
        export_materials="EXPORT",
    )

    arm["height_m"] = 1.82
    for t in arm.animation_data.nla_tracks:
        t.mute = False

    new_doc = json.loads(temp_gltf.read_text(encoding="utf-8"))
    base_doc = json.loads((BACKUP_DIR / "chartest.gltf").read_text(encoding="utf-8"))

    shutil.copy2(BACKUP_DIR / "chartest.bin", CHARTEST_DIR / "chartest.bin")
    if (BACKUP_DIR / "chartest_walk.bin").exists():
        shutil.copy2(BACKUP_DIR / "chartest_walk.bin", CHARTEST_DIR / "chartest_walk.bin")

    buf_offset = len(base_doc["buffers"])
    view_offset = len(base_doc["bufferViews"])
    acc_offset = len(base_doc["accessors"])

    assert len(new_doc["buffers"]) == 1
    mesh_bin_name = "chartest_mesh_hd.bin"
    shutil.copy2(stage_dir / new_doc["buffers"][0]["uri"], CHARTEST_DIR / mesh_bin_name)
    new_doc["buffers"][0]["uri"] = mesh_bin_name
    base_doc["buffers"].extend(new_doc["buffers"])

    for view in new_doc["bufferViews"]:
        view["buffer"] += buf_offset
    base_doc["bufferViews"].extend(new_doc["bufferViews"])

    for acc in new_doc["accessors"]:
        if "bufferView" in acc:
            acc["bufferView"] += view_offset
        if "sparse" in acc:
            acc["sparse"]["indices"]["bufferView"] += view_offset
            acc["sparse"]["values"]["bufferView"] += view_offset
    base_doc["accessors"].extend(new_doc["accessors"])

    old_joint_names = [base_doc["nodes"][i]["name"] for i in base_doc["skins"][0]["joints"]]
    new_joint_names = [new_doc["nodes"][i]["name"] for i in new_doc["skins"][0]["joints"]]
    joint_map = [old_joint_names.index(n) for n in new_joint_names]

    processed_joints = set()
    for mesh in new_doc["meshes"]:
        for prim in mesh["primitives"]:
            ai = prim["attributes"].get("JOINTS_0")
            if ai is not None and ai not in processed_joints:
                processed_joints.add(ai)
                acc = base_doc["accessors"][ai + acc_offset]
                view = base_doc["bufferViews"][acc["bufferView"]]
                bpath = CHARTEST_DIR / base_doc["buffers"][view["buffer"]]["uri"]
                data = bytearray(bpath.read_bytes())
                fmt = {5121: "B", 5123: "H"}[acc["componentType"]]
                size = struct.calcsize(fmt)
                stride = view.get("byteStride", size * 4)
                start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
                for row in range(acc["count"]):
                    for c in range(4):
                        off = start + row * stride + c * size
                        val = struct.unpack_from("<" + fmt, data, off)[0]
                        struct.pack_into("<" + fmt, data, off, joint_map[val])
                bpath.write_bytes(data)

            prim["attributes"] = {k: v + acc_offset for k, v in prim["attributes"].items()}
            if "indices" in prim:
                prim["indices"] += acc_offset
            for target in prim.get("targets", []):
                for k in list(target.keys()):
                    target[k] += acc_offset
            prim["material"] = 0

    shape_catalog = [
        {
            "name": kb.name,
            "min": round(kb.slider_min, 2),
            "max": round(kb.slider_max, 2),
            "internal": kb.name.startswith("_"),
        }
        for kb in obj.data.shape_keys.key_blocks[1:]
    ]

    # Register Female texture & M_Char_Female material slot + KHR_materials_variants on runtime glTF
    female_img_idx = len(base_doc.setdefault("images", []))
    base_doc["images"].append({"mimeType": "image/png", "name": "Chartest-Female-v01-128", "uri": sex_meta["female"]["texture"]})

    sampler_idx = base_doc["textures"][0].get("sampler", 0) if base_doc.get("textures") else 0
    female_tex_idx = len(base_doc.setdefault("textures", []))
    base_doc["textures"].append({"sampler": sampler_idx, "source": female_img_idx})

    male_mat = json.loads(json.dumps(base_doc["materials"][0]))
    female_mat = json.loads(json.dumps(male_mat))
    female_mat["name"] = "M_Char_Female"
    female_mat.setdefault("pbrMetallicRoughness", {})["baseColorTexture"] = {"index": female_tex_idx}
    female_mat_idx = len(base_doc["materials"])
    base_doc["materials"].append(female_mat)

    ext_used = base_doc.setdefault("extensionsUsed", [])
    if "KHR_materials_variants" not in ext_used:
        ext_used.append("KHR_materials_variants")
    base_doc.setdefault("extensions", {})["KHR_materials_variants"] = {
        "variants": [{"name": "male"}, {"name": "female"}]
    }
    for mesh in new_doc["meshes"]:
        for prim in mesh["primitives"]:
            prim.setdefault("extensions", {})["KHR_materials_variants"] = {
                "mappings": [
                    {"material": 0, "variants": [0]},
                    {"material": female_mat_idx, "variants": [1]},
                ]
            }

    base_doc["meshes"] = new_doc["meshes"]
    base_doc["asset"]["generator"] = "Grimora tools/blender/author_chartest_customization.py"
    base_doc.setdefault("extras", {})["customization"] = {
        "shape_keys": shape_catalog,
        "sex_variants": sex_meta,
        "height_controller": height_meta,
    }
    (CHARTEST_DIR / "chartest.gltf").write_text(json.dumps(base_doc, indent=1) + "\n", encoding="utf-8")


def render_customization_showcases(blend_path, out_dir):
    """Render visual lineups of Body Archetypes, Isolated Muscle/Segment Sliders, and Height 0.4m-2.6m."""
    out_dir.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.open_mainfile(filepath=str(blend_path))
    scene = bpy.context.scene
    obj = bpy.data.objects["chartest_body"]
    arm = bpy.data.objects["chartest_rig"]

    # Mute NLA tracks so we see clean symmetrical standing A-pose silhouettes
    for t in arm.animation_data.nla_tracks:
        t.mute = True
    arm.animation_data.action = None

    # Verify live `is_female`, `breasts_size`, and `glutes_size` drivers on Shape Keys & Material!
    depsgraph = bpy.context.evaluated_depsgraph_get()
    base_mat = obj.data.materials[0]
    for flag, b_sz, g_sz, exp_body, exp_bust, exp_glute, exp_mix in [
        (False, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        (True, 1.45, 0.85, 1.0, 1.45, 0.85, 1.0),
    ]:
        arm["is_female"] = flag
        arm["breasts_size"] = b_sz
        arm["glutes_size"] = g_sz
        arm.update_tag()
        obj.update_tag()
        base_mat.node_tree.update_tag()
        depsgraph.update()
        ev_sk = obj.data.shape_keys.evaluated_get(depsgraph)
        ev_nt = base_mat.node_tree.evaluated_get(depsgraph)
        got_body = ev_sk.key_blocks["Feminine_Body"].value
        got_bust = ev_sk.key_blocks["Female_Breasts"].value
        got_glute = ev_sk.key_blocks["Glutes_Mass"].value
        got_mix = ev_nt.nodes["Sex_Tex_Switch"].inputs[0].default_value
        print(
            f"[sex-check] is_female={flag} -> Feminine_Body={got_body:.2f}, "
            f"Female_Breasts={got_bust:.2f}, Glutes_Mass={got_glute:.2f}, TexMix={got_mix:.2f}"
        )
        assert abs(got_body - exp_body) < 1e-4
        assert abs(got_bust - exp_bust) < 1e-4
        assert abs(got_glute - exp_glute) < 1e-4
        assert abs(got_mix - exp_mix) < 1e-4
    arm["is_female"] = False
    arm["breasts_size"] = 1.0
    arm["glutes_size"] = 0.0

    def make_psx_mat(mname, png_name):
        img = bpy.data.images.load(str(CHARTEST_DIR / png_name), check_existing=True)
        m = bpy.data.materials.new(mname)
        m.use_nodes = True
        nodes = m.node_tree.nodes
        links = m.node_tree.links
        nodes.clear()
        out = nodes.new("ShaderNodeOutputMaterial")
        bsdf = nodes.new("ShaderNodeBsdfPrincipled")
        bsdf.inputs["Roughness"].default_value = 0.85
        if "Specular IOR Level" in bsdf.inputs:
            bsdf.inputs["Specular IOR Level"].default_value = 0.08
        tex = nodes.new("ShaderNodeTexImage")
        tex.image = img
        tex.interpolation = "Closest"
        links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
        links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
        return m

    mat_male = make_psx_mat("Showcase_Male", "Chartest-Grimora-v02-128.png")
    mat_female = make_psx_mat("Showcase_Female", "Chartest-Female-v01-128.png")

    def bake_snapshot(
        name,
        x_pos,
        height_m=1.82,
        sk_values=None,
        yaw_deg=0.0,
        is_female=False,
        breasts_size=1.0,
        glutes_size=0.0,
    ):
        driven_keys = ("Feminine_Body", "Female_Breasts", "Glutes_Mass")
        for kb in obj.data.shape_keys.key_blocks[1:]:
            if not kb.name.startswith("_") and kb.name not in driven_keys:
                kb.value = 0.0
        g_val = float(glutes_size)
        b_val = float(breasts_size)
        if sk_values:
            for k, v in sk_values.items():
                if k == "Glutes_Mass":
                    g_val = float(v)
                elif k == "Female_Breasts":
                    b_val = float(v)
                elif k not in driven_keys:
                    obj.data.shape_keys.key_blocks[k].value = v
        arm["is_female"] = bool(is_female)
        arm["breasts_size"] = b_val
        arm["glutes_size"] = g_val
        arm["height_m"] = float(height_m)
        arm.update_tag()
        obj.update_tag()
        depsgraph.update()
        eval_obj = obj.evaluated_get(depsgraph)
        mesh_copy = bpy.data.meshes.new_from_object(eval_obj, preserve_all_data_layers=True, depsgraph=depsgraph)
        mesh_copy.materials.clear()
        mesh_copy.materials.append(mat_female if is_female else mat_male)
        snap = bpy.data.objects.new(name, mesh_copy)
        scene.collection.objects.link(snap)
        snap.location = (x_pos, 0.0, 0.0)
        snap.rotation_euler = (0.0, 0.0, math.radians(yaw_deg))
        return snap

    # Hide original rig/body during lineup renders
    obj.hide_render = True
    arm.hide_render = True

    world = bpy.data.worlds.new("ShowcaseWorld")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.052, 0.060, 0.075, 1.0)
    bg.inputs[1].default_value = 1.0

    for lname, loc, energy, col in [
        ("Key", (-2.2, 3.8, 3.2), 4.2, (1.0, 0.97, 0.92)),
        ("Fill", (2.6, 2.8, 2.0), 2.1, (0.82, 0.88, 1.0)),
        ("Rim", (0.0, -3.2, 2.6), 2.2, (0.95, 0.95, 1.0)),
    ]:
        ldata = bpy.data.lights.new(lname, type="SUN")
        ldata.energy = energy
        ldata.color = col
        lobj = bpy.data.objects.new(lname, ldata)
        scene.collection.objects.link(lobj)
        lobj.location = loc
        lobj.rotation_euler = (Vector((0, 0, 1.0)) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()

    # Subtle ground floor bar + height reference ticks
    bm_floor = bmesh.new()
    bmesh.ops.create_grid(bm_floor, x_segments=1, y_segments=1, size=6.0)
    floor_mesh = bpy.data.meshes.new("Floor")
    bm_floor.to_mesh(floor_mesh)
    bm_floor.free()
    floor_obj = bpy.data.objects.new("Floor", floor_mesh)
    scene.collection.objects.link(floor_obj)
    floor_obj.location = (0.0, 0.0, -0.002)
    fmat = bpy.data.materials.new("FloorMat")
    fmat.use_nodes = True
    fmat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.08, 0.09, 0.11, 1.0)
    fmat.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.92
    floor_obj.data.materials.append(fmat)

    cam_data = bpy.data.cameras.new("ShowcaseCam")
    cam = bpy.data.objects.new("ShowcaseCam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam

    eevee_engines = [e.identifier for e in bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items]
    scene.render.engine = "BLENDER_EEVEE_NEXT" if "BLENDER_EEVEE_NEXT" in eevee_engines else "BLENDER_EEVEE"

    def clear_snaps(snaps):
        for s in snaps:
            m = s.data
            bpy.data.objects.remove(s, do_unlink=True)
            if m.users == 0:
                bpy.data.meshes.remove(m)

    # --- SHEET 1: Body Archetypes (Left to Right as viewed from +Y -> +X is Left, -X is Right!)
    archetypes = [
        ("Gaunt", 2.50, 1.80, {"Body_Gaunt_Lean": 1.0}, 18, False, 0.95),
        ("Feminine", 1.25, 1.70, {}, -26, True, 0.95),
        ("Base", 0.00, 1.82, {}, 14, False, 0.95),
        ("Muscular", -1.25, 1.86, {"Body_Muscle": 1.0}, -18, False, 0.95),
        ("Heavy_Fat", -2.55, 1.80, {"Body_Heavy_Fat": 1.0}, -24, False, 0.95),
    ]
    snaps = [bake_snapshot(*a) for a in archetypes]
    scene.render.resolution_x = 1600
    scene.render.resolution_y = 780
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = 6.75
    cam.location = (0.0, 5.0, 0.98)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out_dir / "showcase_archetypes.png")
    bpy.ops.render.render(write_still=True)
    clear_snaps(snaps)

    # --- SHEET 1B: Dedicated `is_female` Toggle & Female Variety Lineup (3/4 angle so bust & glute depth read clearly)
    female_lineup = [
        ("Male_Default", 2.15, 1.82, {}, 24, False, 0.0, 0.0),
        ("Female_Default", 0.72, 1.70, {}, -36, True, 1.00, 0.0),
        ("Female_Athletic", -0.72, 1.78, {"Body_Muscle": 0.65, "Legs_Full": 0.35}, -38, True, 0.85, 0.45),
        ("Female_Stout", -2.15, 1.62, {"Body_Heavy_Fat": 0.75}, -40, True, 1.35, 0.65),
    ]
    snaps = [bake_snapshot(*f) for f in female_lineup]
    cam_data.ortho_scale = 5.75
    scene.render.filepath = str(out_dir / "showcase_female_toggle.png")
    bpy.ops.render.render(write_still=True)
    clear_snaps(snaps)

    # --- SHEET 1C: Dedicated `breasts_size` (0.0 .. 2.00) & `glutes_size` (-0.55 .. 1.90) Slider Progression!
    bust_glute_lineup = [
        ("Bust_00", 2.95, 1.72, {}, -54, True, 0.00, 0.0),
        ("Bust_06", 1.98, 1.72, {}, -54, True, 0.65, 0.0),
        ("Bust_13", 1.00, 1.72, {}, -54, True, 1.30, 0.0),
        ("Bust_20", 0.02, 1.72, {}, -54, True, 2.00, 0.0),
        ("Glute_Flat", -1.02, 1.72, {}, -126, True, 1.00, -0.55),
        ("Glute_Std", -1.98, 1.72, {}, -126, True, 1.00, 0.35),
        ("Glute_Max", -2.95, 1.72, {}, -126, True, 1.00, 1.90),
    ]
    snaps = [bake_snapshot(*bg_item) for bg_item in bust_glute_lineup]
    cam_data.ortho_scale = 7.45
    scene.render.filepath = str(out_dir / "showcase_breasts_glutes.png")
    bpy.ops.render.render(write_still=True)
    clear_snaps(snaps)

    # --- SHEET 1D: Zoomed-in Torso & Pelvis Profile Close-up (3/4 Front, Pure Side -88°, 3/4 Back -126°)
    closeup_lineup = [
        ("Close_Med_34", 1.15, 1.76, {}, -48, True, 1.00, 0.35),
        ("Close_Full_34", 0.38, 1.76, {}, -56, True, 1.85, 1.45),
        ("Close_Profile", -0.38, 1.76, {}, -88, True, 1.85, 1.55),
        ("Close_Back_34", -1.15, 1.76, {}, -126, True, 1.70, 1.65),
    ]
    snaps = [bake_snapshot(*c_item) for c_item in closeup_lineup]
    cam.location = (0.0, 5.0, 1.12)
    cam.rotation_euler = (Vector((0.0, 0.0, 1.10)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    cam_data.ortho_scale = 3.25
    scene.render.filepath = str(out_dir / "showcase_anatomy_closeup.png")
    bpy.ops.render.render(write_still=True)
    clear_snaps(snaps)
    cam.location = (0.0, 5.0, 0.98)
    cam.rotation_euler = (Vector((0.0, 0.0, 0.94)) - cam.location).to_track_quat("-Z", "Y").to_euler()

    # --- SHEET 2: Regional Muscle/Segment Isolation (Shoulders/Chest/Arms, Belly, Glutes/Thighs/Calves)
    regions = [
        ("Slim_Limbs", 2.60, 1.82, {"Arms_Upper": -0.65, "Forearms_Mass": -0.65, "Legs_Full": -0.65, "Shoulders_Width": -0.55}, 14),
        ("Upper_Power", 1.30, 1.82, {"Shoulders_Width": 1.0, "Traps_Neck": 1.0, "Chest_Pecs": 1.0, "Torso_V_Taper": 1.0, "Arms_Upper": 1.0, "Forearms_Mass": 1.0}, 16),
        ("Belly_Waist", 0.00, 1.82, {"Abdomen_Belly": 1.0, "Waist_Width": 0.8}, -28),
        ("Thighs_Hips", -1.30, 1.82, {"Hips_Width": 1.0, "Glutes_Mass": 1.0, "Thighs_Mass": 1.0}, -42),
        ("Calves_Back", -2.60, 1.82, {"Calves_Mass": 1.0, "Glutes_Mass": 0.8, "Torso_V_Taper": 0.8}, 155),
    ]
    snaps = [bake_snapshot(*r) for r in regions]
    cam_data.ortho_scale = 7.05
    scene.render.filepath = str(out_dir / "showcase_regions.png")
    bpy.ops.render.render(write_still=True)
    clear_snaps(snaps)

    # --- SHEET 3: Proportional Height Lineup (0.40m, 0.85m, 1.35m, 1.82m, 2.20m, 2.60m) with Horizontal Meter Bars!
    bar_mat = bpy.data.materials.new("MeterBarMat")
    bar_mat.use_nodes = True
    bar_mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.20, 0.30, 0.42, 1.0)
    bar_mat.node_tree.nodes["Principled BSDF"].inputs["Emission Color"].default_value = (0.18, 0.28, 0.40, 1.0)
    if "Emission Strength" in bar_mat.node_tree.nodes["Principled BSDF"].inputs:
        bar_mat.node_tree.nodes["Principled BSDF"].inputs["Emission Strength"].default_value = 0.55

    bars = []
    for h_mark in (0.40, 0.85, 1.35, 1.82, 2.20, 2.60):
        bm_b = bmesh.new()
        bmesh.ops.create_cube(bm_b, size=1.0)
        bmesh_data = bpy.data.meshes.new(f"Bar_{h_mark}")
        bm_b.to_mesh(bmesh_data)
        bm_b.free()
        bobj = bpy.data.objects.new(f"Bar_{h_mark}", bmesh_data)
        scene.collection.objects.link(bobj)
        bobj.location = (0.0, -0.35, h_mark)
        bobj.scale = (6.8, 0.008, 0.006)
        bobj.data.materials.append(bar_mat)
        bars.append(bobj)

    heights = [
        ("H_040", 2.85, 0.40, {}, 10),
        ("H_085", 2.15, 0.85, {}, 10),
        ("H_135", 1.25, 1.35, {}, 10),
        ("H_182", 0.15, 1.82, {}, 10),
        ("H_220", -1.20, 2.20, {"Body_Muscle": 0.35}, -10),
        ("H_260", -2.85, 2.60, {"Body_Muscle": 0.65, "Shoulders_Width": 0.45}, -12),
    ]
    snaps = [bake_snapshot(*h) for h in heights]
    scene.render.resolution_x = 1600
    scene.render.resolution_y = 860
    cam_data.ortho_scale = 7.95
    cam.location = (-0.05, 5.5, 1.32)
    cam.rotation_euler = (Vector((-0.05, 0.0, 1.30)) - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = str(out_dir / "showcase_heights.png")
    bpy.ops.render.render(write_still=True)
    clear_snaps(snaps)
    clear_snaps(bars)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--preview-dir", type=Path, required=True)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else [])

    # Archive old raw chartest_source.blend so assets/models/chartest only keeps the clean upgraded asset
    legacy_src = CHARTEST_DIR / "chartest_source.blend"
    if legacy_src.exists():
        archive_dst = CHARTEST_DIR / "authoring_archive/chartest_source.legacy.blend"
        archive_dst.parent.mkdir(parents=True, exist_ok=True)
        if not archive_dst.exists():
            shutil.move(str(legacy_src), str(archive_dst))
        else:
            legacy_src.unlink()
        print("[custom] archived legacy chartest_source.blend -> authoring_archive/chartest_source.legacy.blend")

    bpy.ops.wm.open_mainfile(filepath=str(CHARTEST_DIR / "chartest.blend"))
    bpy.context.preferences.filepaths.save_version = 0

    obj = bpy.data.objects["chartest_body"]
    arm = bpy.data.objects["chartest_rig"]

    clear_customization(obj, arm)
    author_anatomical_shape_keys(obj, arm)
    sex_meta = author_sex_parameter(obj, arm)
    height_meta = author_proportional_height_system(obj, arm)
    height_checks = validate_height_and_shapes(obj, arm)

    bpy.ops.wm.save_as_mainfile(filepath=str(CHARTEST_DIR / "chartest.blend"), compress=True)
    transplant_with_morphs_to_gltf(obj, arm, height_meta, sex_meta, args.preview_dir / "stage_morphs")

    total_keys = len(obj.data.shape_keys.key_blocks) - 1
    user_keys = [kb.name for kb in obj.data.shape_keys.key_blocks[1:] if not kb.name.startswith("_")]
    print(f"[custom] authored {len(user_keys)} anatomical Shape Keys (+2 height allometry keys = {total_keys} morph targets): {user_keys}")
    print(f"[custom] height checks: {json.dumps(height_checks)}")

    render_customization_showcases(CHARTEST_DIR / "chartest.blend", args.preview_dir)
    print("[custom] all showcases rendered!")


if __name__ == "__main__":
    main()
