"""Import a user-supplied character without rebuilding its mesh or skin weights.

Run with Blender 5.2:
    blender --background --python tools/blender/import_chartest.py -- --source <chartest.blend>

The original .blend and the legacy wanderer are read-only inputs. Existing pose clips
are preserved; when the Mixamo source exists, its native idle replaces the procedural
idle override, which is moved to authoring_archive rather than discarded.
"""
import argparse
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys

import bpy
from mathutils import Matrix, Quaternion, Vector

ROOT = Path(__file__).resolve().parents[2]
LEGACY = ROOT / 'assets/models/wanderer/wanderer.gltf'
OUTPUT = ROOT / 'assets/models/chartest'
MIXAMO_IDLE = ROOT / 'assets/animations/mixamo/breathing_idle.fbx'
SWORD = ROOT / 'assets/models/weapons/espada_reta.glb'
NAMES = {
    'root_01': 'root', 'pelvis_02': 'hips',
    'spine_01_07': 'spine', 'spine_02_08': 'chest',
    'neck_021': 'neck', 'head_022': 'head',
}
for side, suffixes in [('l', ['03', '04', '05', '06', '09', '010', '011', '012', '00', '018', '019']),
                       ('r', ['036', '037', '038', '039', '023', '024', '025', '026', '027', '033', '034'])]:
    for prefix, suffix, target in zip(
        ['thigh', 'calf', 'foot', 'toe', 'clavicle', 'arm', 'forearm', 'hand', 'thumb_01', 'fingers_01', 'fingers_02'],
        suffixes,
        ['thigh', 'shin', 'foot', 'toe', 'clavicle', 'upper_arm', 'forearm', 'hand', 'thumb', 'fingers', 'fingertips'],
    ):
        NAMES[f'{prefix}_{side}_{suffix}'] = f'{target}.{side.upper()}'


def normalize():
    """Source FBX is Y-up, faces +Z, and has right on -X. glTF export expects Blender Z-up."""
    arm = next(o for o in bpy.context.scene.objects if o.type == 'ARMATURE')
    meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o.find_armature() == arm]
    assert meshes, 'No skinned body found'
    # A proper rotation, not a reflection: preserves winding and the source's skinning.
    basis = Matrix(((-1, 0, 0, 0), (0, 0, 1, 0), (0, 1, 0, 0), (0, 0, 0, 1)))
    transforms = {o: basis @ o.matrix_world for o in [arm, *meshes]}
    for o, transform in transforms.items():
        o.parent = None
        o.matrix_world = transform
    bpy.ops.object.select_all(action='DESELECT')
    for o in transforms:
        o.select_set(True)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)
    for mesh in meshes:
        world = mesh.matrix_world.copy()
        mesh.parent = arm
        mesh.matrix_world = world
    for old, new in NAMES.items():
        assert old in arm.data.bones, old
        arm.data.bones[old].name = new
        # Blender renames the deform groups with the bone; handle versions which do not.
        for mesh in meshes:
            group = mesh.vertex_groups.get(old)
            if group:
                group.name = new
    bpy.ops.object.mode_set(mode='EDIT')
    bones = arm.data.edit_bones
    bones['root'].parent = None
    for bone in list(bones):
        if bone.name == '_rootJoint' or bone.name.startswith('ik_'):
            bones.remove(bone)
    # Keep the native hand -> index1 -> index2 -> index3 chains. A grouped finger
    # is not a valid parent for a separately weighted index finger.
    hand = bones['hand.R'].head
    fingers = bones['fingers.R'].head
    direction = (fingers-hand).normalized()
    # The thumb is on the blade's side. The palm's inward side is perpendicular to it.
    palm = Vector((0, 1, 0)).cross(direction).normalized()
    grip = fingers + direction * 0.002 + palm * 0.028
    weapon = bones.new('weapon.R')
    weapon.head = grip; weapon.tail = grip + Vector((0, 0.20, 0))
    weapon.parent = bones['hand.R']; weapon.use_deform = False
    lantern = bones.new('lantern')
    lantern.head = Vector((-0.235, 0.06, bones['hips'].head.z))
    lantern.tail = lantern.head + Vector((0, 0, -0.11))
    lantern.parent = bones['hips']; lantern.use_deform = False
    bpy.ops.object.mode_set(mode='OBJECT')
    arm.name = 'chartest_rig'
    for mesh in meshes:
        mesh.name = 'chartest_body'
    for o in list(bpy.context.scene.objects):
        if o not in [arm, *meshes]:
            bpy.data.objects.remove(o, do_unlink=True)
    return arm, meshes


def blend_right_wrist(arm, meshes):
    """Give coincident hand/forearm seam vertices the same smooth joint blend."""
    wrist = arm.data.bones['hand.R'].head_local
    direction = (wrist - arm.data.bones['forearm.R'].head_local).normalized()
    changed = 0
    for mesh in meshes:
        into_armature = arm.matrix_world.inverted() @ mesh.matrix_world
        hand = mesh.vertex_groups.get('hand.R')
        forearm = mesh.vertex_groups.get('forearm.R')
        if not hand or not forearm:
            continue
        for vertex in mesh.data.vertices:
            weights = {g.group: g.weight for g in vertex.groups}
            # A duplicated thumb-side wrist ring incorrectly carries a little thumb
            # weight. Include it so coincident seam vertices receive identical blends.
            if weights.get(hand.index, 0) + weights.get(forearm.index, 0) < 0.60:
                continue
            offset = into_armature @ vertex.co - wrist
            along = offset.dot(direction)
            radial = (offset - direction*along).length
            if not (-0.030 < along < 0.030 and radial < 0.045):
                continue
            t = max(0.0, min(1.0, (along + 0.030)/0.060))
            influence = t*t*(3-2*t)
            for group in mesh.vertex_groups:
                if group.index in weights:
                    group.remove([vertex.index])
            hand.add([vertex.index], influence, 'REPLACE')
            forearm.add([vertex.index], 1-influence, 'REPLACE')
            changed += 1
    print(f'[hand-grip] right wrist seam: {changed} vertices share a smooth hand/forearm blend')
    return changed


def retarget(arm, grip):
    """Carry the existing non-pose clips over as rotations from the bind pose."""
    existing = set(bpy.context.scene.objects)
    bpy.context.scene.render.fps = 60
    bpy.ops.import_scene.gltf(filepath=str(LEGACY))
    imported = set(bpy.context.scene.objects) - existing
    source = next(o for o in imported if o.type == 'ARMATURE')
    source_rest = {b.name: (source.matrix_world @ b.matrix_local).to_quaternion() for b in source.data.bones}
    target_rest = {b.name: b.matrix_local.to_quaternion() for b in arm.data.bones}
    scene = bpy.context.scene
    scene.render.fps = 60
    source.animation_data.action = None
    for track in source.animation_data.nla_tracks:
        track.mute = True
    arm.animation_data_create()
    animations = json.loads(LEGACY.read_text(encoding='utf-8'))['animations']
    # Sword clips come from the editable RONs. These seven clips provide jump, hurt and death too.
    for animation in animations:
        name = animation['name']
        if name.startswith('sword_'):
            continue
        track = next(t for t in source.animation_data.nla_tracks if t.name == name)
        strip = track.strips[0]
        action = strip.action
        source.animation_data.action = action
        source.animation_data.action_slot = strip.action_slot
        duration = (strip.action_frame_end - strip.action_frame_start) / 60.0
        target_action = bpy.data.actions.new('chartest_' + name)
        arm.animation_data.action = target_action
        previous = {}
        for frame in range(round(duration * 60) + 1):
            scene.frame_set(round(strip.action_frame_start) + frame)
            delta = {b.name: ((source.matrix_world @ b.matrix).to_quaternion() @ source_rest[b.name].inverted()) for b in source.pose.bones}
            held = delta['hand.R']
            for grip_joint, rotation in grip['deltas'].items():
                delta[grip_joint] = held @ rotation
            delta['weapon.R'] = held @ grip['weapon_rotation'] @ target_rest['weapon.R'].inverted()
            for bone in arm.pose.bones:
                parent_delta = delta.get(bone.parent.name, Quaternion()) if bone.parent else Quaternion()
                own_delta = delta.get(bone.name, parent_delta)
                q = target_rest[bone.name].inverted() @ parent_delta.inverted() @ own_delta @ target_rest[bone.name]
                if bone.name in previous and previous[bone.name].dot(q) < 0:
                    q = -q
                previous[bone.name] = q
                bone.rotation_mode = 'QUATERNION'
                bone.rotation_quaternion = q
                bone.keyframe_insert('rotation_quaternion', frame=frame)
                bone.location = Vector()
                if bone.name in ('root', 'hips'):
                    bone.location = target_rest[bone.name].inverted() @ source_rest[bone.name] @ source.pose.bones[bone.name].location
                    bone.keyframe_insert('location', frame=frame)
        target_track = arm.animation_data.nla_tracks.new()
        target_track.name = name
        target_track.strips.new(name, 0, target_action)
        target_track.mute = True
        arm.animation_data.action = None
        print(f'[chartest] retargeted {name}: {duration:.3f}s')
    source.animation_data.action = None
    for o in imported:
        bpy.data.objects.remove(o, do_unlink=True)
    for track in arm.animation_data.nla_tracks:
        track.mute = False
    for bone in arm.pose.bones:
        bone.matrix_basis = Matrix.Identity(4)
    return {a['name']: a.get('extras') for a in animations}


def add_grip_reference(arm, grip):
    """A hidden native reference lets the CPU pose baker reuse this exact fitted hand."""
    rest = {b.name: b.matrix_local.to_quaternion() for b in arm.data.bones}
    delta = {b.name: Quaternion() for b in arm.data.bones}
    delta.update(grip['deltas'])
    delta['weapon.R'] = grip['weapon_rotation'] @ rest['weapon.R'].inverted()
    for track in arm.animation_data.nla_tracks:
        track.mute = True
    action = bpy.data.actions.new('__grip_right')
    arm.animation_data.action = action
    for bone in arm.pose.bones:
        above = delta[bone.parent.name] if bone.parent else Quaternion()
        bone.rotation_mode = 'QUATERNION'
        bone.rotation_quaternion = rest[bone.name].inverted() @ above.inverted() @ delta[bone.name] @ rest[bone.name]
        bone.location = Vector()
        for at in (0, 1):
            bone.keyframe_insert('rotation_quaternion', frame=at)
            bone.keyframe_insert('location', frame=at)
    track = arm.animation_data.nla_tracks.new()
    track.name = '__grip_right'
    track.strips.new('__grip_right', 0, action)
    arm.animation_data.action = None
    for track in arm.animation_data.nla_tracks:
        track.mute = False
    for bone in arm.pose.bones:
        bone.matrix_basis = Matrix.Identity(4)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, required=True)
    args = parser.parse_args(sys.argv[sys.argv.index('--')+1:])
    bpy.ops.wm.open_mainfile(filepath=str(args.source.resolve()))
    bpy.context.preferences.filepaths.save_version = 0
    arm, meshes = normalize()
    wrist_blend_vertices = blend_right_wrist(arm, meshes)
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from hand_grip import fit_right_hand
    grip = fit_right_hand(arm, meshes, SWORD)
    for bone in arm.pose.bones:
        bone.matrix_basis = Matrix.Identity(4)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    socket = arm.data.edit_bones['weapon.R']
    socket.head = grip['center']
    socket.tail = socket.head + Vector((0, 0.20, 0))
    bpy.ops.object.mode_set(mode='OBJECT')
    extras = retarget(arm, grip)
    add_grip_reference(arm, grip)
    extras['__grip_right'] = dict(source='Grimora cylinder contact fit', hidden=True)
    native_idle = MIXAMO_IDLE.exists()
    if native_idle:
        from mixamo_idle import add_mixamo_idle
        extras['sword_idle'] = add_mixamo_idle(arm, MIXAMO_IDLE, grip)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    grip_summary = dict(grip.get('metrics', {}))
    grip_summary.update(center_m=list(grip['center']),
                        weapon_axis=list(grip['weapon_rotation'] @ Vector((0, 1, 0))),
                        joints=list(grip['deltas']), sword=str(SWORD.relative_to(ROOT)),
                        wrist_blend_vertices=wrist_blend_vertices)
    (OUTPUT/'right_hand_grip.json').write_text(json.dumps(grip_summary, indent=2)+'\n', encoding='utf-8')
    bpy.context.scene.frame_set(0)
    bpy.ops.object.select_all(action='DESELECT')
    for o in [arm, *meshes]:
        o.select_set(True)
    bpy.context.view_layer.objects.active = arm
    path = OUTPUT / 'chartest.gltf'
    bpy.ops.export_scene.gltf(filepath=str(path), export_format='GLTF_SEPARATE', use_selection=True,
        export_yup=True, export_animations=True, export_animation_mode='NLA_TRACKS',
        export_force_sampling=True, export_skins=True, export_def_bones=False,
        export_materials='EXPORT', export_cameras=False, export_lights=False)
    document = json.loads(path.read_text(encoding='utf-8'))
    for a in document.get('animations', []):
        if extras.get(a['name']):
            a['extras'] = extras[a['name']]
    document['asset']['generator'] = 'Grimora tools/blender/import_chartest.py'
    path.write_text(json.dumps(document, indent=1)+'\n', encoding='utf-8')
    dest = OUTPUT / 'chartest_clips'
    dest.mkdir(exist_ok=True)
    for clip in LEGACY.with_name('wanderer_clips').glob('*.ron'):
        if native_idle and clip.name == 'sword_idle.ron':
            continue
        if not (dest/clip.name).exists():
            shutil.copy2(clip, dest/clip.name)
    if native_idle and (dest/'sword_idle.ron').exists():
        archive = OUTPUT / 'authoring_archive'
        archive.mkdir(exist_ok=True)
        backup = archive/'sword_idle.procedural.ron'
        index = 1
        while backup.exists():
            backup = archive/f'sword_idle.procedural.{index}.ron'
            index += 1
        shutil.move(str(dest/'sword_idle.ron'), backup)
    bpy.ops.wm.save_as_mainfile(filepath=str(OUTPUT/'chartest.blend'), compress=True)
    # Locomotion is authored directly against the exported bind skeleton. Run it
    # last so a reimport cannot restore the previous character's RON override.
    subprocess.run(['python', str(ROOT/'tools/author_chartest_walk.py')], check=True)
    print(f'[chartest] exported {sum(len(m.data.polygons) for m in meshes)} faces, {len(arm.data.bones)} bones; clips: {[a["name"] for a in document.get("animations",[])]}')


if __name__ == '__main__':
    main()
