"""Bake the official Mixamo Breathing Idle onto chartest's existing bind skeleton.

Used by import_chartest.py when assets/animations/mixamo/breathing_idle.fbx exists.
Body motion comes from the FBX, with fixed bind calibration and scaled hip travel.
Only the prop orientation and the right-hand grip are adapted to the sword.
"""
import math

import bpy
from mathutils import Matrix, Quaternion, Vector

NAME = 'sword_idle'
MAPPING = {
    'hips': 'Hips', 'spine': 'Spine', 'chest': 'Spine2', 'neck': 'Neck', 'head': 'Head',
}
for side, word in [('L', 'Left'), ('R', 'Right')]:
    for target, source in [('clavicle', 'Shoulder'), ('upper_arm', 'Arm'), ('forearm', 'ForeArm'),
                           ('hand', 'Hand'), ('thigh', 'UpLeg'), ('shin', 'Leg'),
                           ('foot', 'Foot'), ('toe', 'ToeBase')]:
        MAPPING[f'{target}.{side}'] = word + source


def frame(direction, pole):
    y = direction.normalized()
    z = (pole - y * pole.dot(y)).normalized()
    x = y.cross(z).normalized()
    return Matrix((x, y, x.cross(y))).transposed().to_quaternion()


def add_mixamo_idle(target, path, grip):
    scene = bpy.context.scene
    scene.render.fps = 60
    existing = set(scene.objects)
    bpy.ops.import_scene.fbx(filepath=str(path))
    imported = set(scene.objects) - existing
    source = next(o for o in imported if o.type == 'ARMATURE')
    source.matrix_world = Matrix.Rotation(math.pi, 4, 'Z') @ source.matrix_world
    source_action = source.animation_data.action
    first, end = map(int, source_action.frame_range)
    count = end - first + 1
    source_bone = {b.name.split(':')[-1]: b for b in source.data.bones}
    source_pose = {b.name.split(':')[-1]: b for b in source.pose.bones}
    source_rest = {name: (source.matrix_world @ b.matrix_local).to_quaternion() for name, b in source_bone.items()}
    source_head = {name: source.matrix_world @ b.head_local for name, b in source_bone.items()}
    rest = {b.name: b.matrix_local.to_quaternion() for b in target.data.bones}
    head = {b.name: b.head_local.copy() for b in target.data.bones}
    calibration = {b.name: Quaternion() for b in target.data.bones}
    for side, word in [('L', 'Left'), ('R', 'Right')]:
        for native, child, mixamo, mixamo_child in [
            ('upper_arm', 'forearm', 'Arm', 'ForeArm'),
            ('forearm', 'hand', 'ForeArm', 'Hand'),
        ]:
            a = head[f'{child}.{side}'] - head[f'{native}.{side}']
            b = source_head[word+mixamo_child] - source_head[word+mixamo]
            calibration[f'{native}.{side}'] = a.rotation_difference(b)
        native_fingers = head[f'fingers.{side}'] - head[f'hand.{side}']
        index = word + 'HandIndex1'
        source_fingers = (source_head[index] - source_head[word+'Hand']) if index in source_head else (
            source_head[word+'Hand'] - source_head[word+'ForeArm'])
        calibration[f'hand.{side}'] = native_fingers.rotation_difference(source_fingers)
        pinky = word + 'HandPinky1'
        native_index = next((b for b in target.data.bones if b.name.startswith(f'index_01_{side.lower()}_')), None)
        if index in source_head and pinky in source_head and native_index:
            native_row = native_index.head_local - head[f'fingers.{side}']
            source_row = source_head[index] - source_head[pinky]
            # Match finger direction AND thumb side. A single-axis alignment leaves
            # the wrist roll ambiguous and can turn the palm backwards.
            calibration[f'hand.{side}'] = frame(source_fingers, source_row) @ frame(native_fingers, native_row).inverted()
    # One fitted bind-space hand is carried by the captured wrist motion. Its
    # independent finger chains and the prop share the same anatomical grip frame.
    weapon_reference = grip['weapon_rotation'] @ rest['weapon.R'].inverted()
    source_hips_rest = source_head['Hips']
    height_scale = head['hips'].z / source_hips_rest.z
    for track in target.animation_data.nla_tracks:
        track.mute = True
    target.animation_data.action = None
    samples = []
    forearm_roll = None
    resting_blade = Vector((0.75, 0.60, -0.28)).normalized()
    for k in range(count):
        scene.frame_set(first+k)
        source_delta = {name: (source.matrix_world @ bone.matrix).to_quaternion() @ source_rest[name].inverted()
                        for name, bone in source_pose.items()}
        base_forearm = source_delta['RightForeArm'] @ calibration['forearm.R']
        base_hand = source_delta['RightHand'] @ calibration['hand.R']
        axis = (base_forearm @ (head['hand.R'] - head['forearm.R'])).normalized()
        if forearm_roll is None:
            blade = base_hand @ grip['weapon_rotation'] @ Vector((0, 1, 0))
            a = (blade-axis*blade.dot(axis)).normalized()
            b = (resting_blade-axis*resting_blade.dot(axis)).normalized()
            forearm_roll = math.atan2(axis.dot(a.cross(b)), a.dot(b))
        pronation = Quaternion(axis, forearm_roll)
        delta = {}
        for bone in target.data.bones:
            above = delta.get(bone.parent.name, Quaternion()) if bone.parent else Quaternion()
            if bone.name in MAPPING:
                delta[bone.name] = source_delta[MAPPING[bone.name]] @ calibration[bone.name]
                if bone.name in ('forearm.R', 'hand.R'):
                    # Pronation moves forearm and hand together around the elbow-to-wrist
                    # axis. Their relative wrist rotation remains exactly as captured.
                    delta[bone.name] = pronation @ delta[bone.name]
            elif bone.name in grip['deltas']:
                delta[bone.name] = delta['hand.R'] @ grip['deltas'][bone.name]
            elif bone.name == 'weapon.R':
                delta[bone.name] = delta['hand.R'] @ weapon_reference
            else:
                delta[bone.name] = above
        local = {}
        for bone in target.data.bones:
            above = delta[bone.parent.name] if bone.parent else Quaternion()
            local[bone.name] = rest[bone.name].inverted() @ above.inverted() @ delta[bone.name] @ rest[bone.name]
        hip_world = source.matrix_world @ source_pose['Hips'].matrix.translation
        hip_shift = (hip_world - source_hips_rest) * height_scale
        samples.append([local, rest['hips'].inverted() @ hip_shift])
    # Short seam blend keeps the downloaded motion continuous when the engine loops it.
    seam_frames = min(24, count//8)
    for k in range(count-seam_frames, count):
        u = (k-(count-seam_frames))/(seam_frames-1)
        weight = u*u*(3-2*u)
        rotations, shift = samples[k]
        for name in rotations:
            rotations[name] = rotations[name].slerp(samples[0][0][name], weight)
        samples[k][1] = shift.lerp(samples[0][1], weight)
    action = bpy.data.actions.new('Mixamo_Breathing_Idle')
    target.animation_data.action = action
    previous = {}
    meshes = [o for o in scene.objects if o.type == 'MESH' and o.find_armature() == target]
    ground_shifts = []
    # One vertical translation keeps the retargeted body's soles at ground level without
    # changing any captured joint rotations, limb lengths or the horizontal hip sway.
    for k, (rotations, shift) in enumerate(samples):
        for bone in target.pose.bones:
            bone.rotation_mode = 'QUATERNION'
            q = rotations[bone.name]
            if bone.name in previous and previous[bone.name].dot(q) < 0:
                q = -q
            previous[bone.name] = q
            bone.rotation_quaternion = q
            bone.location = shift if bone.name == 'hips' else Vector()
        bpy.context.view_layer.update()
        depsgraph = bpy.context.evaluated_depsgraph_get()
        lowest = min((o.matrix_world @ v.co).z for o in meshes for v in o.evaluated_get(depsgraph).data.vertices)
        ground_shifts.append(-lowest)
        target.pose.bones['hips'].location += rest['hips'].inverted() @ Vector((0, 0, -lowest))
        for bone in target.pose.bones:
            bone.keyframe_insert('rotation_quaternion', frame=k)
            if bone.name in ('root', 'hips'):
                bone.keyframe_insert('location', frame=k)
    track = target.animation_data.nla_tracks.new()
    track.name = NAME
    track.strips.new(NAME, 0, action)
    target.animation_data.action = None
    source.animation_data.action = None
    for o in imported:
        bpy.data.objects.remove(o, do_unlink=True)
    for track in target.animation_data.nla_tracks:
        track.mute = False
    for bone in target.pose.bones:
        bone.matrix_basis = Matrix.Identity(4)
    print(f'[mixamo] Breathing Idle: {count} frames, {(count-1)/60:.3f}s, hip scale {height_scale:.3f}, '
          f'ground adjustment {min(ground_shifts):.4f}..{max(ground_shifts):.4f}m')
    return dict(source='Adobe Mixamo', motion='Breathing Idle', fps=60)
