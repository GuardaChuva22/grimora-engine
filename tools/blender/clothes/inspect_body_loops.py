"""Inspect chartest_body vertex groups, polygon structure, and arm/neck/hip cross-sections."""
import bpy
from mathutils import Vector

CHARTEST_BLEND = r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\chartest.blend"
bpy.ops.wm.open_mainfile(filepath=CHARTEST_BLEND)

body = bpy.data.objects["chartest_body"]
arm = bpy.data.objects["chartest_rig"]
basis = body.data.shape_keys.key_blocks[0].data
pts = [basis[i].co.copy() for i in range(len(body.data.vertices))]

print(f"Total verts={len(pts)}, polys={len(body.data.polygons)}")
poly_sizes = {}
for p in body.data.polygons:
    poly_sizes[len(p.vertices)] = poly_sizes.get(len(p.vertices), 0) + 1
print("Poly vertex counts:", poly_sizes)

# Measure left arm (-X) along upper_arm.L and forearm.L
for bname in ["clavicle.L", "upper_arm.L", "forearm.L"]:
    b = arm.data.bones[bname]
    a, t = b.head_local, b.tail_local
    u = (t - a).normalized()
    L = (t - a).length
    for frac in [0.0, 0.33, 0.66, 1.0]:
        c = a + u * (L * frac)
        near = [p for p in pts if p.x < -0.08 and abs((p - c).dot(u)) < 0.024 and (p - c).length < 0.11]
        if near:
            rads = [((p - c) - u * (p - c).dot(u)).length for p in near]
            print(f"  {bname:12s} f={frac:.2f} c={[round(x,4) for x in c]}: r_min={min(rads):.4f} r_mean={sum(rads)/len(rads):.4f} r_max={max(rads):.4f} (n={len(near)})")

# Measure neck base around z=1.50..1.56
neck_pts = [p for p in pts if 1.50 <= p.z <= 1.56 and abs(p.x) < 0.12]
print(f"Neck base (z 1.50..1.56): x=[{min(p.x for p in neck_pts):.4f}..{max(p.x for p in neck_pts):.4f}], y=[{min(p.y for p in neck_pts):.4f}..{max(p.y for p in neck_pts):.4f}]")

# Check symmetry of chartest_body
pos_x = sum(1 for p in pts if p.x > 0.001)
neg_x = sum(1 for p in pts if p.x < -0.001)
zero_x = sum(1 for p in pts if abs(p.x) <= 0.001)
print(f"Symmetry check: +X={pos_x}, -X={neg_x}, center={zero_x}")
