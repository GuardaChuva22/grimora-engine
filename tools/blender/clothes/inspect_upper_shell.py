"""Check upper-body quad subset of chartest_body (collar, wrists, waist boundary loops)."""
import bpy
import bmesh
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

fa_l = arm.data.bones["forearm.L"]
fa_r = arm.data.bones["forearm.R"]
nk = arm.data.bones["neck"]

def keep_face(f):
    c = f.calc_center_median()
    # Exclude lower body below waist z=0.995
    if c.z < 0.995:
        return False
    # Exclude head & upper neck
    if abs(c.x) < 0.11 and c.z > 1.515:
        return False
    # Exclude hands past wrist (t > 0.92 along forearm)
    for fa in (fa_l, fa_r):
        u = (fa.tail_local - fa.head_local).normalized()
        L = (fa.tail_local - fa.head_local).length
        t = (c - fa.head_local).dot(u) / L
        if abs(c.x) > 0.35 and t > 0.92:
            return False
    return True

del_faces = [f for f in bm.faces if not keep_face(f)]
bmesh.ops.delete(bm, geom=del_faces, context="FACES")
del_loose = [v for v in bm.verts if not v.link_faces]
bmesh.ops.delete(bm, geom=del_loose, context="VERTS")

# Find connected boundary loops
bnd_edges = [e for e in bm.edges if e.is_boundary]
print(f"Kept verts={len(bm.verts)}, faces={len(bm.faces)}, boundary_edges={len(bnd_edges)}")

visited = set()
loops = []
for e in bnd_edges:
    if e in visited:
        continue
    comp = []
    stack = [e]
    while stack:
        cur = stack.pop()
        if cur in visited:
            continue
        visited.add(cur)
        comp.append(cur)
        for v in cur.verts:
            for ne in v.link_edges:
                if ne.is_boundary and ne not in visited:
                    stack.append(ne)
    vset = {v for ed in comp for v in ed.verts}
    cx = sum(v.co.x for v in vset) / len(vset)
    cy = sum(v.co.y for v in vset) / len(vset)
    cz = sum(v.co.z for v in vset) / len(vset)
    zmin = min(v.co.z for v in vset)
    zmax = max(v.co.z for v in vset)
    print(f"  Loop edges={len(comp)}, verts={len(vset)}, center=({cx:.3f},{cy:.3f},{cz:.3f}), z=[{zmin:.4f}..{zmax:.4f}]")
