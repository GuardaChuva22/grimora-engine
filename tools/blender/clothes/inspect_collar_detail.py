"""Print exact vertices of the 18-vert collar loop and 22-vert waist loop."""
import bpy, bmesh, math

CHARTEST_BLEND = r"C:\Users\Admin\Documents\! VIBECODING\GRIMORA ENGINE\assets\models\chartest\chartest.blend"
bpy.ops.wm.open_mainfile(filepath=CHARTEST_BLEND)
body = bpy.data.objects["chartest_body"]
arm = bpy.data.objects["chartest_rig"]
basis = body.data.shape_keys.key_blocks[0].data

bm = bmesh.new()
bm.from_mesh(body.data)
for i, v in enumerate(bm.verts):
    v.co = basis[i].co.copy()

fa_l = arm.data.bones["forearm.L"]
fa_r = arm.data.bones["forearm.R"]

def keep_face(f):
    c = f.calc_center_median()
    if c.z < 0.995: return False
    if abs(c.x) < 0.11 and c.z > 1.515: return False
    for fa in (fa_l, fa_r):
        u = (fa.tail_local - fa.head_local).normalized()
        L = (fa.tail_local - fa.head_local).length
        if abs(c.x) > 0.35 and (c - fa.head_local).dot(u) / L > 0.92:
            return False
    return True

bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep_face(f)], context="FACES")
bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")

collar_vs = sorted(
    [v for v in bm.verts if v.is_boundary and v.co.z > 1.45],
    key=lambda v: math.atan2(v.co.x, v.co.y + 0.09)
)
print(f"Collar verts ({len(collar_vs)}):")
for v in collar_vs:
    print(f"  v={v.index:4d} pos=({v.co.x:+.4f}, {v.co.y:+.4f}, {v.co.z:.4f})")

print("\nFront centerline verts remaining (|x|<0.001, y>-0.04, z>1.30):")
for v in sorted([v for v in bm.verts if abs(v.co.x) < 0.001 and v.co.y > -0.04 and v.co.z > 1.30], key=lambda v: -v.co.z):
    print(f"  v={v.index:4d} pos=({v.co.x:+.4f}, {v.co.y:+.4f}, {v.co.z:.4f}) bnd={v.is_boundary}")

waist_vs = sorted(
    [v for v in bm.verts if v.is_boundary and v.co.z < 1.05],
    key=lambda v: math.atan2(v.co.x, v.co.y + 0.03)
)
print(f"\nWaist verts ({len(waist_vs)}):")
for v in waist_vs:
    print(f"  v={v.index:4d} pos=({v.co.x:+.4f}, {v.co.y:+.4f}, {v.co.z:.4f})")
