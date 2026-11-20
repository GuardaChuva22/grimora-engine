"""Test snug neck-base collar boundary using neck+head weights / cylinder radius."""
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

def is_neck_or_head(c):
    # Neck axis sits around (0.0, -0.088)
    r_neck = math.hypot(c.x, (c.y + 0.088) * 0.85)
    return c.z > 1.560 or (c.z > 1.522 and r_neck < 0.078)

def keep_face(f):
    c = f.calc_center_median()
    if c.z < 1.015: return False
    if is_neck_or_head(c): return False
    for fa in (fa_l, fa_r):
        u = (fa.tail_local - fa.head_local).normalized()
        L = (fa.tail_local - fa.head_local).length
        if abs(c.x) > 0.35 and (c - fa.head_local).dot(u) / L > 0.92:
            return False
    return True

bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep_face(f)], context="FACES")
bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")

collar_vs = [v for v in bm.verts if v.is_boundary and v.co.z > 1.45]
waist_vs = [v for v in bm.verts if v.is_boundary and v.co.z < 1.08]
print(f"Collar count={len(collar_vs)}: x=[{min(v.co.x for v in collar_vs):.4f}..{max(v.co.x for v in collar_vs):.4f}], y=[{min(v.co.y for v in collar_vs):.4f}..{max(v.co.y for v in collar_vs):.4f}], z=[{min(v.co.z for v in collar_vs):.4f}..{max(v.co.z for v in collar_vs):.4f}]")
for v in sorted(collar_vs, key=lambda v: math.atan2(v.co.x, v.co.y + 0.09)):
    print(f"  c v={v.index:4d} ({v.co.x:+.4f}, {v.co.y:+.4f}, {v.co.z:.4f})")
print(f"Waist count={len(waist_vs)}: z=[{min(v.co.z for v in waist_vs):.4f}..{max(v.co.z for v in waist_vs):.4f}]")
for v in sorted(waist_vs, key=lambda v: math.atan2(v.co.x, v.co.y + 0.03)):
    print(f"  w v={v.index:4d} ({v.co.x:+.4f}, {v.co.y:+.4f}, {v.co.z:.4f})")
