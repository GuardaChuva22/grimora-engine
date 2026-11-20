"""Find exact clean quad-ring boundaries on chartest_body for collar, wrists, and waist."""
import bpy
import bmesh
from collections import Counter
from mathutils import Vector

CHARTEST_BLEND = r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\chartest.blend"
bpy.ops.wm.open_mainfile(filepath=CHARTEST_BLEND)

body = bpy.data.objects["chartest_body"]
arm = bpy.data.objects["chartest_rig"]
basis = body.data.shape_keys.key_blocks[0].data

bm = bmesh.new()
bm.from_mesh(body.data)
for i, v in enumerate(bm.verts):
    v.co = basis[i].co.copy()
bm.verts.ensure_lookup_table()
bm.edges.ensure_lookup_table()
bm.faces.ensure_lookup_table()

# Check horizontal edge loops around the torso (z in 0.90..1.15)
print("=== TORSO RING CANDIDATES ===")
# Find center-front vertices (x ~ 0, y > 0) sorted by z
cf_verts = sorted([v for v in bm.verts if abs(v.co.x) < 0.001 and v.co.y > 0.0 and 0.85 <= v.co.z <= 1.60], key=lambda v: v.co.z)
for v in cf_verts:
    print(f"  center-front vert idx={v.index:4d}: z={v.co.z:.4f}, y={v.co.y:.4f}")

cb_verts = sorted([v for v in bm.verts if abs(v.co.x) < 0.001 and v.co.y < 0.0 and 0.85 <= v.co.z <= 1.60], key=lambda v: v.co.z)
print("=== CENTER-BACK VERTS ===")
for v in cb_verts:
    print(f"  center-back  vert idx={v.index:4d}: z={v.co.z:.4f}, y={v.co.y:.4f}")

# Check forearm axial stations along forearm.L
fa = arm.data.bones["forearm.L"]
a, b = fa.head_local, fa.tail_local
u = (b - a).normalized()
L = (b - a).length
print(f"\n=== FOREARM.L (len={L:.4f}) VERTEX STATIONS ===")
arm_vs = [v for v in bm.verts if v.co.x < -0.32]
# Cluster by projection t along forearm.L
ts = sorted([(v.co - a).dot(u) / L for v in arm_vs])
# Print histogram buckets
for b_i in range(-2, 14):
    lo, hi = b_i * 0.1, (b_i + 1) * 0.1
    cnt = sum(1 for t in ts if lo <= t < hi)
    if cnt:
        vals = [t for t in ts if lo <= t < hi]
        print(f"  t in [{lo:5.2f}, {hi:5.2f}): count={cnt:3d}, mean_t={sum(vals)/len(vals):.3f}")
