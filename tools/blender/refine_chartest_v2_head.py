"""Refine only v2 facial topology; preserve the runtime rig and all native clips.

Run with Blender --background --python this_file. Outputs stay in chartest_v2.
"""
import bpy, bmesh, json, math, shutil, struct
from pathlib import Path
from mathutils import Vector
from mathutils.geometry import delaunay_2d_cdt, closest_point_on_tri, barycentric_transform

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'assets/models/chartest_v2'
STAGE = OUT / 'authoring'
STAGE.mkdir(exist_ok=True)
BASE = ROOT / 'assets/models/chartest/chartest.blend'
bpy.ops.wm.open_mainfile(filepath=str(BASE))
bpy.context.preferences.filepaths.save_version = 0
obj = next(o for o in bpy.context.scene.objects if o.type == 'MESH')
arm = next(o for o in bpy.context.scene.objects if o.type == 'ARMATURE')
for b in arm.pose.bones:
    b.matrix_basis.identity()
for t in arm.animation_data.nla_tracks:
    t.mute = True
arm.animation_data.action = None
bm = bmesh.new(); bm.from_mesh(obj.data)
bm.verts.ensure_lookup_table(); bm.faces.ensure_lookup_table()
deform = bm.verts.layers.deform.verify()
head_group = obj.vertex_groups['head'].index
head = [v for v in bm.verts if v[deform].get(head_group, 0) > .5]
# Weld only the head's coincident UV-seam vertices; loop UVs stay independent.
bmesh.ops.remove_doubles(bm, verts=head, dist=1e-6)
patch = [f for f in bm.faces if all(v.co.z > 1.60 for v in f.verts)
         and sum(v.co.y for v in f.verts)/len(f.verts) > -.008]
uv_layer = bm.loops.layers.uv.active
samples = [([v.co.copy() for v in f.verts], [l[uv_layer].uv.copy() for l in f.loops]) for f in patch]
edges = [e for e in bm.edges if sum(f in patch for f in e.link_faces) == 1]
boundary = []
current = edges[0].verts[0]
remaining = set(edges)
while remaining:
    boundary.append(current)
    edge = next(e for e in remaining if current in e.verts)
    remaining.remove(edge); current = edge.other_vert(current)
assert current == boundary[0] and len(boundary) > 8, 'The face patch must have one closed perimeter'
fixed = [v for v in bm.verts if v not in {v for f in patch for v in f.verts}]
fixed_coords = {v: v.co.copy() for v in fixed}

def sample(x, z):
    q = Vector((x, z, 0)); best = None
    for coords, uvs in samples:
        flat = [Vector((v.x, v.z, 0)) for v in coords]
        c = closest_point_on_tri(q, *flat); dist = (c-q).length_squared
        if best is None or dist < best[0]:
            depth = barycentric_transform(c, *flat, *[Vector((v.y, 0, 0)) for v in coords]).x
            uv = barycentric_transform(c, *flat, *[Vector((u.x, u.y, 0)) for u in uvs])
            best = dist, depth, Vector((uv.x, uv.y))
    return best[1:]

coords = [Vector((v.co.x, v.co.z)) for v in boundary]
vertices = list(boundary)
constraints = [(i, (i+1)%len(boundary)) for i in range(len(boundary))]
ring_ids = {}; eyes = []; mouth = []
def add(x, z, y=None):
    depth, _ = sample(x, z)
    v = bm.verts.new((x, depth if y is None else y, z)); v[deform][head_group] = 1.
    coords.append(Vector((x,z))); vertices.append(v)
    return len(vertices)-1

def ring(name, cx, cz, rx, rz, n, depth):
    ids = []
    for i in range(n):
        t = 2*math.pi*i/n
        x = cx + rx*math.cos(t)
        z = cz + rz*math.sin(t) * (.85 + .15*abs(math.sin(t)))
        ids.append(add(x,z,depth(x,z,t)))
    constraints.extend((ids[i],ids[(i+1)%n]) for i in range(n))
    ring_ids[name] = ids
    return ids

for side in [-1,1]:
    cx, cz = side*.032, 1.714
    ey = sample(cx,cz)[0] - .014
    eyes.append((cx,ey,cz))
    surface = lambda x,z: ey + .013*math.sqrt(max(.03,1-((x-cx)/.016)**2-((z-cz)/.012)**2))
    ring(f'eye_outer_{side}',cx,cz,.022,.014,12,lambda x,z,t:sample(x,z)[0]-.001)
    ring(f'eye_lid_{side}',cx,cz,.016,.008,12,lambda x,z,t:max(surface(x,z)+.003,sample(x,z)[0]+.0005))
    ids=ring(f'eye_open_{side}',cx,cz,.0135,.0054,12,lambda x,z,t:surface(x,z)+.0015)

for name,rx,rz,push in [('mouth_outer',.034,.013,0),('mouth_lip',.026,.0052,.002),('mouth_open',.023,.001,0)]:
    ids=ring(name,0,1.636,rx,rz,16,lambda x,z,t:sample(x,z)[0]+push)
    if name=='mouth_open': mouth=ids
# Deliberate nasal bridge, sidewalls, wings and tip, instead of global subdivision.
nose_rows=[]
for z,half,depth in [(1.716,.008,.046),(1.692,.0085,.049),(1.670,.010,.062),(1.658,.014,.061),(1.651,.013,.043)]:
    row=[add(-half,z,depth-.014),add(0,z,depth),add(half,z,depth-.014)]
    constraints.extend([(row[0],row[1]),(row[1],row[2])]); nose_rows.append(row)
for a,b in zip(nose_rows,nose_rows[1:]):
    constraints.extend(zip(a,b))
verts2,_,tris,orig,_,_ = delaunay_2d_cdt(coords,constraints,[],0,1e-8)
mapped=[]
for xy,ids in zip(verts2,orig):
    assert ids, 'Unexpected crossing in facial constraints'
    mapped.append(vertices[ids[0]])

def inside(q, loop):
    hit=False
    for a,b in zip(loop,loop[1:]+loop[:1]):
        if (a.y>q.y)!=(b.y>q.y) and q.x<(b.x-a.x)*(q.y-a.y)/(b.y-a.y)+a.x:
            hit=not hit
    return hit
holes=[ring_ids[f'eye_open_{s}'] for s in [-1,1]]+[mouth]
bmesh.ops.delete(bm, geom=patch, context='FACES_ONLY')
def face(vs,material=0):
    f=bm.faces.new(vs); f.material_index=material
    for loop in f.loops:
        _,uv=sample(loop.vert.co.x,loop.vert.co.z); loop[uv_layer].uv=uv
    return f
for tri in tris:
    q=sum((verts2[i] for i in tri),Vector((0,0)))/len(tri)
    if inside(q,coords[:len(boundary)]) and not any(inside(q,[coords[i] for i in h]) for h in holes):
        face([mapped[i] for i in tri])

def material(name,color):
    mat=bpy.data.materials.new(name); mat.diffuse_color=(*color,1); mat.use_nodes=True
    bs=mat.node_tree.nodes.get('Principled BSDF'); bs.inputs['Base Color'].default_value=(*color,1)
    bs.inputs['Roughness'].default_value=.82
    obj.data.materials.append(mat); return len(obj.data.materials)-1
white=material('Eyes_sclera',(.75,.70,.57)); iris=material('Eyes_iris',(.16,.105,.05))
pupil=material('Eyes_pupil',(.012,.009,.007)); cavity=material('Mouth_interior',(.052,.018,.016))
# A shallow closed cavity keeps the mouth usable when opened without seeing through the head.
back=[]
for i in mouth:
    co=vertices[i].co; v=bm.verts.new((co.x*.92,co.y-.021,1.636+(co.z-1.636)*2))
    v[deform][head_group]=1; back.append(v)
for i in range(len(mouth)):
    n=(i+1)%len(mouth); face([vertices[mouth[i]],back[i],back[n],vertices[mouth[n]]],cavity)
face(list(reversed(back)),cavity)
# Low polygon eyeballs remain behind the eyelids, with independent iris/pupil materials.
eye_verts={}
for side,(cx,cy,cz) in zip([-1,1],eyes):
    rings=[]
    for angle in [0,.20,.43,.85,1.25,1.70,2.2,math.pi]:
        row=[]
        for i in range(12):
            t=2*math.pi*i/12
            v=bm.verts.new((cx+.016*math.sin(angle)*math.cos(t),cy+.013*math.cos(angle),cz+.012*math.sin(angle)*math.sin(t)))
            v[deform][head_group]=1; row.append(v)
        rings.append(row)
    for k in range(len(rings)-1):
        for i in range(12):
            n=(i+1)%12
            face([rings[k][i],rings[k][n],rings[k+1][n],rings[k+1][i]],pupil if k==0 else iris if k==1 else white)
    eye_verts[side]=[v for row in rings for v in row]
# Remove only orphaned vertices from the replaced surface and degenerate sphere poles.
bmesh.ops.remove_doubles(bm,verts=[v for vs in eye_verts.values() for v in vs],dist=1e-7)
for v in list(bm.verts):
    if not v.link_faces: bm.verts.remove(v)
bmesh.ops.recalc_face_normals(bm,faces=list(bm.faces))
assert all((v.co-co).length<1e-8 for v,co in fixed_coords.items()), 'Non-facial geometry changed'
assert not [f for f in bm.faces if f.calc_area()<1e-12], 'Degenerate base faces'
bm.to_mesh(obj.data); bm.free(); obj.data.update()
obj.shape_key_add(name='Basis')
base=[v.co.copy() for v in obj.data.vertices]
def key(name,fn):
    shape=obj.shape_key_add(name=name)
    for i,co in enumerate(base): shape.data[i].co=fn(co.copy())
    return shape
def gauss(x,c,r): return math.exp(-((x-c)/r)**2*2)
def deform_key(name,fn):
    def apply(co):
        if co.z<1.60: return co
        return fn(co)
    return key(name,apply)
deform_key('Nose_Width',lambda v:Vector((v.x*(1+.32*gauss(v.x,0,.021)*gauss(v.z,1.666,.044)*max(0,min(1,(v.y+.005)/.035))),v.y,v.z)))
deform_key('Nose_Projection',lambda v:Vector((v.x,v.y+.012*gauss(v.x,0,.017)*gauss(v.z,1.672,.035)*max(0,min(1,v.y/.025)),v.z)))
deform_key('Nose_Bridge',lambda v:Vector((v.x,v.y+.008*gauss(v.x,0,.013)*gauss(v.z,1.702,.027)*max(0,min(1,v.y/.025)),v.z)))
deform_key('Eyes_Spacing',lambda v:Vector((v.x+(.007 if v.x>0 else -.007)*gauss(abs(v.x),.032,.024)*gauss(v.z,1.714,.025)*max(0,min(1,(v.y+.02)/.03)),v.y,v.z)))
deform_key('Eyes_Size',lambda v:Vector((v.x+(v.x-math.copysign(.032,v.x))*.22*gauss(abs(v.x),.032,.025)*gauss(v.z,1.714,.025)*max(0,min(1,(v.y+.02)/.03)),v.y,v.z+(v.z-1.714)*.22*gauss(abs(v.x),.032,.025)*gauss(v.z,1.714,.025)*max(0,min(1,(v.y+.02)/.03)))))
deform_key('Mouth_Width',lambda v:Vector((v.x*(1+.25*gauss(v.x,0,.046)*gauss(v.z,1.636,.020)*max(0,min(1,(v.y+.02)/.03))),v.y,v.z)))
deform_key('Lips_Fullness',lambda v:Vector((v.x,v.y+.004*gauss(v.x,0,.029)*gauss(v.z,1.636,.008)*max(0,min(1,(v.y+.005)/.025)),v.z)))
deform_key('Jaw_Width',lambda v:Vector((v.x*(1+.16*gauss(v.z,1.628,.039)),v.y,v.z)))
deform_key('Chin_Length',lambda v:Vector((v.x,v.y,v.z-.010*gauss(v.x,0,.045)*gauss(v.z,1.615,.025)*max(0,min(1,(v.y+.02)/.04)))))
deform_key('Cheekbones',lambda v:Vector((v.x*(1+.10*gauss(v.z,1.68,.025)),v.y+.004*gauss(abs(v.x),.053,.024)*gauss(v.z,1.68,.025),v.z)))
deform_key('Head_Width',lambda v:Vector((v.x*(1+.12*min(1,max(0,(v.z-1.60)/.065))),v.y,v.z)))
deform_key('Head_Height',lambda v:Vector((v.x,v.y,1.60+(v.z-1.60)*1.07)))
for side in [-1,1]:
    cx,cy,cz=eyes[0 if side==-1 else 1]
    shape=obj.shape_key_add(name='Blink_'+('L' if side>0 else 'R'))
    for i,v in enumerate(base):
        co=v.copy()
        if abs(v.x-cx)<.018 and abs(v.z-cz)<.011 and v.y>cy+.008:
            # Move only skin; eyeball materials identify the independent eye geometry.
            mats={p.material_index for p in obj.data.polygons if i in p.vertices}
            if mats=={0}:
                w=max(0,min(1,(.011-abs(v.z-cz))/.005))
                co.z=v.z+(cz-v.z)*w
                co.y=cy+.013*math.sqrt(max(.03,1-((co.x-cx)/.016)**2-((co.z-cz)/.012)**2))+.0009
        shape.data[i].co=co
deform_key('Mouth_Open',lambda v:Vector((v.x,v.y-.003*gauss(v.x,0,.04)*gauss(v.z,1.624,.025),v.z-.016*gauss(v.x,0,.04)*gauss(v.z,1.626,.026)*max(0,min(1,(1.636-v.z)/.004))*max(0,min(1,(v.y+.025)/.04)))))
for name in ['Nose_Width','Nose_Projection','Eyes_Spacing','Mouth_Width','Jaw_Width','Head_Width','Head_Height']:
    obj.data.shape_keys.key_blocks[name].slider_min=-1
for t in arm.animation_data.nla_tracks: t.mute=False
bpy.ops.object.select_all(action='DESELECT'); obj.select_set(True); arm.select_set(True)
bpy.context.view_layer.objects.active=obj
bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'chartest_v2.blend'),compress=True)
# Export mesh only, then transplant it onto the unchanged runtime document.
temp=STAGE/'head_geometry.gltf'
bpy.ops.export_scene.gltf(filepath=str(temp),export_format='GLTF_SEPARATE',use_selection=True,
    export_animations=False,export_skins=True,export_def_bones=False,export_morph=True,
    export_morph_normal=False,export_morph_tangent=False,export_materials='EXPORT')
new=json.loads(temp.read_text()); old=json.loads((OUT/'chartest_v2.gltf').read_text())
# Always replace from the original runtime baseline to keep generation repeatable.
old=json.loads((ROOT/'assets/models/chartest/chartest.gltf').read_text())
for b in old['buffers']:
    if b.get('uri','').startswith('chartest'): b['uri']=b['uri'].replace('chartest','chartest_v2',1)
buf_offset=len(old['buffers']); view_offset=len(old['bufferViews']); acc_offset=len(old['accessors'])
for i,b in enumerate(new['buffers']):
    name=f'chartest_v2_head{str(i) if i else ""}.bin'; shutil.copy2(temp.parent/b['uri'],OUT/name); b['uri']=name
old['buffers'].extend(new['buffers'])
for view in new['bufferViews']: view['buffer']+=buf_offset
old['bufferViews'].extend(new['bufferViews'])
for acc in new['accessors']:
    if 'bufferView' in acc: acc['bufferView']+=view_offset
    if 'sparse' in acc:
        acc['sparse']['indices']['bufferView']+=view_offset
        acc['sparse']['values']['bufferView']+=view_offset
old['accessors'].extend(new['accessors'])
old_names=[old['nodes'][i]['name'] for i in old['skins'][0]['joints']]
new_names=[new['nodes'][i]['name'] for i in new['skins'][0]['joints']]
joint_map=[old_names.index(n) for n in new_names]
processed_joints=set()
for mesh in new['meshes']:
    for prim in mesh['primitives']:
        ai=prim['attributes'].get('JOINTS_0')
        if ai is not None and ai not in processed_joints:
            processed_joints.add(ai)
            acc=old['accessors'][ai+acc_offset]; view=old['bufferViews'][acc['bufferView']]
            bpath=OUT/old['buffers'][view['buffer']]['uri']; data=bytearray(bpath.read_bytes())
            fmt={5121:'B',5123:'H'}[acc['componentType']]; size=struct.calcsize(fmt); stride=view.get('byteStride',size*4)
            start=view.get('byteOffset',0)+acc.get('byteOffset',0)
            for row in range(acc['count']):
                for c in range(4):
                    off=start+row*stride+c*size; val=struct.unpack_from('<'+fmt,data,off)[0]
                    struct.pack_into('<'+fmt,data,off,joint_map[val])
            bpath.write_bytes(data)
        prim['attributes']={k:v+acc_offset for k,v in prim['attributes'].items()}
        if 'indices' in prim: prim['indices']+=acc_offset
        for target in prim.get('targets',[]):
            for k in target: target[k]+=acc_offset
        mi=prim.get('material',0)
        if mi:
            mat=new['materials'][mi]
            if mat not in old['materials']: old['materials'].append(mat)
            prim['material']=old['materials'].index(mat)
old['meshes']=new['meshes']; old['asset']['generator']='Grimora v2 local facial retopology'
old.setdefault('extras',{})['head_customization']={'source':'chartest_v2.blend','morphs':len(obj.data.shape_keys.key_blocks)-1,'runtime_morph_support':False}
(OUT/'chartest_v2.gltf').write_text(json.dumps(old,indent=1)+'\n')
report={'base_triangles':880,'triangles':sum(len(p.vertices)-2 for p in obj.data.polygons),
    'vertices':len(obj.data.vertices),'shape_keys':[k.name for k in obj.data.shape_keys.key_blocks],
    'body_preserved':True,'rig_preserved':old['skins']==json.loads((ROOT/'assets/models/chartest/chartest.gltf').read_text())['skins'],
    'native_animations_preserved':len(old['animations'])}
(OUT/'head_validation.json').write_text(json.dumps(report,indent=2)+'\n'); print('HEAD_RESULT',json.dumps(report))
# Fast studio previews of the actual editable model, with a neutral material display.
for mat in obj.data.materials:
    if mat==obj.data.materials[0]: mat.diffuse_color=(.40,.25,.17,1)
scene=bpy.context.scene; scene.render.engine='BLENDER_WORKBENCH'
scene.display.shading.light='STUDIO'; scene.display.shading.studiolight_rotate_z=.5
scene.display.shading.color_type='MATERIAL'; scene.display.shading.show_shadows=True
scene.display.shading.show_cavity=False
scene.display.shading.background_type='WORLD'; scene.world.color=(.045,.055,.07)
scene.render.resolution_x=640; scene.render.resolution_y=640; scene.render.resolution_percentage=100
camera=bpy.data.objects.new('Head_preview_camera',bpy.data.cameras.new('Head_preview_camera')); scene.collection.objects.link(camera)
scene.camera=camera; camera.data.type='ORTHO'; camera.data.ortho_scale=.32
target=Vector((0,-.028,1.703))
for name,pos in [('front',(0,.8,1.703)),('quarter',(.52,.66,1.715)),('profile',(.8,-.028,1.703))]:
    camera.location=pos; camera.rotation_euler=(target-camera.location).to_track_quat('-Z','Y').to_euler()
    scene.render.filepath=str(OUT/f'head_{name}.png'); bpy.ops.render.render(write_still=True)
camera.location=(0,.8,1.703); camera.rotation_euler=(target-camera.location).to_track_quat('-Z','Y').to_euler()
for name,values in [('variant',{'Head_Width':.6,'Jaw_Width':.6,'Nose_Width':.7,'Nose_Projection':.4,'Eyes_Spacing':-.35,'Eyes_Size':.35,'Mouth_Width':-.4}),('blink',{'Blink_L':1,'Blink_R':1}),('open',{'Mouth_Open':1})]:
    for k in obj.data.shape_keys.key_blocks: k.value=values.get(k.name,0)
    bpy.context.view_layer.update()
    scene.render.filepath=str(OUT/f'head_{name}.png'); bpy.ops.render.render(write_still=True)
