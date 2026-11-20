"""Measure exact torso, neck, shoulder, and arm cross-sections on chartest.blend."""
import bpy
import math
from mathutils import Vector

CHARTEST_BLEND = r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\chartest.blend"
bpy.ops.wm.open_mainfile(filepath=CHARTEST_BLEND)

body = bpy.data.objects["chartest_body"]
arm = bpy.data.objects["chartest_rig"]

print("=== KEY BONES (Z-up world) ===")
for bname in ["hips", "spine", "chest", "neck", "head", "clavicle.L", "upper_arm.L", "forearm.L", "hand.L", "thigh.L"]:
    b = arm.data.bones[bname]
    print(f"  {bname:14s}: head={[round(v,4) for v in b.head_local]} -> tail={[round(v,4) for v in b.tail_local]} len={b.length:.4f}")

basis = body.data.shape_keys.key_blocks[0].data if body.data.shape_keys else body.data.vertices
pts = [basis[i].co.copy() for i in range(len(body.data.vertices))]

print("\n=== TORSO SLICES (|x| < 0.19) ===")
for z in [0.88, 0.92, 0.96, 1.00, 1.06, 1.12, 1.18, 1.24, 1.30, 1.36, 1.42, 1.48, 1.52, 1.56]:
    sl = [p for p in pts if abs(p.z - z) < 0.022 and abs(p.x) < 0.19]
    if sl:
        xmax = max(abs(p.x) for p in sl)
        ymin = min(p.y for p in sl)
        ymax = max(p.y for p in sl)
        print(f"  z={z:.2f}: xmax={xmax:.4f}, y=[{ymin:.4f} .. {ymax:.4f}]")

print("\n=== LEFT ARM PROFILE ALONG UPPER_ARM.L -> FOREARM.L ===")
ua = arm.data.bones["upper_arm.L"]
fa = arm.data.bones["forearm.L"]
ha = arm.data.bones["hand.L"]
for label, a, b in [("upper_arm.L", ua.head_local, ua.tail_local), ("forearm.L", fa.head_local, fa.tail_local)]:
    ab = b - a
    u = ab.normalized()
    L = ab.length
    for frac in [0.0, 0.25, 0.5, 0.75, 1.0]:
        c = a + u * (L * frac)
        near = [p for p in pts if p.x > 0.12 and abs((p - c).dot(u)) < 0.025 and (p - c).length < 0.11]
        if near:
            rmax = max(((p - c) - u * (p - c).dot(u)).length for p in near)
            rmean = sum(((p - c) - u * (p - c).dot(u)).length for p in near) / len(near)
            print(f"  {label} t={frac:.2f} center={[round(v,4) for v in c]}: r_mean={rmean:.4f}, r_max={rmax:.4f} (n={len(near)})")

print("\n=== SHAPE KEY NAMES ===")
if body.data.shape_keys:
    for idx, kb in enumerate(body.data.shape_keys.key_blocks):
        print(f"  [{idx:2d}] {kb.name} (range {kb.slider_min:.2f}..{kb.slider_max:.2f})")
