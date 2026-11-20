"""Retarget Quaternius (Universal Animation Library) clips onto chartest's native skeleton.

Run from the repository root:
    python tools/import_quaternius_clips.py [--analyze]

Sources live in assets/animations/quaternius/*.glb (UE mannequin skeleton, CC0). Each
combo is one long take; it is cut into three blows at the right hand's slowest points
between its three fastest swings, and every blow gets `swing`, `hit_on`, `hit_off`,
`combo` and `cancel` events. The pelvis' travel on the ground becomes the `root`
track (root motion), so the capsule takes the steps and the pose stays over it.

The clips go into chartest.gltf with their own buffer (chartest_quaternius.bin), after
whatever author_chartest_walk.py wrote, and replace its jump clips. Rerunning removes
what the previous run added. Requires numpy and scipy.
"""
import argparse
import json
from pathlib import Path
import struct
import sys

import numpy as np
from scipy.spatial.transform import Rotation as R, Slerp

ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT / 'assets/models/chartest/chartest.gltf'
SOURCES = ROOT / 'assets/animations/quaternius'
BUFFER = 'chartest_quaternius.bin'
FPS = 60
# Frames over which the pelvis' travel is averaged into root motion.
ROOT_SMOOTHING = 21

# chartest joint <- UE mannequin joint.
MAPPING = {'hips': 'pelvis', 'spine': 'spine_02', 'chest': 'spine_03', 'neck': 'neck_01', 'head': 'Head'}
for side in 'LR':
    s = side.lower()
    for target, source in [('clavicle', 'clavicle'), ('upper_arm', 'upperarm'), ('forearm', 'lowerarm'),
                           ('hand', 'hand'), ('thigh', 'thigh'), ('shin', 'calf'), ('foot', 'foot'),
                           ('toe', 'ball')]:
        MAPPING[f'{target}.{side}'] = f'{source}_{s}'
# Each joint aims at this child, on both skeletons, to line up the bind poses.
AIM = {'hips': ('spine', 'spine_02'), 'spine': ('chest', 'spine_03'), 'chest': ('neck', 'neck_01'),
       'neck': ('head', 'Head')}
# Mixamo joint -> mannequin joint, so one retarget serves both.
MIXAMO = {'Hips': 'pelvis', 'Spine': 'spine_02', 'Spine1': 'spine_mid', 'Spine2': 'spine_03',
          'Neck': 'neck_01', 'Head': 'Head'}
for side, word in (('L', 'Left'), ('R', 'Right')):
    s = side.lower()
    MIXAMO.update({f'{word}Shoulder': f'clavicle_{s}', f'{word}Arm': f'upperarm_{s}',
                   f'{word}ForeArm': f'lowerarm_{s}', f'{word}Hand': f'hand_{s}',
                   f'{word}UpLeg': f'thigh_{s}', f'{word}Leg': f'calf_{s}', f'{word}Foot': f'foot_{s}',
                   f'{word}ToeBase': f'ball_{s}', f'{word}HandIndex1': f'index_01_{s}',
                   f'{word}HandMiddle1': f'middle_01_{s}', f'{word}HandRing1': f'ring_01_{s}'})
    AIM.update({f'clavicle.{side}': (f'upper_arm.{side}', f'upperarm_{s}'),
                f'upper_arm.{side}': (f'forearm.{side}', f'lowerarm_{s}'),
                f'forearm.{side}': (f'hand.{side}', f'hand_{s}'),
                f'thigh.{side}': (f'shin.{side}', f'calf_{s}'),
                f'shin.{side}': (f'foot.{side}', f'foot_{s}'),
                f'foot.{side}': (f'toe.{side}', f'ball_{s}')})

# (source file, clip prefix, seconds of each blow's fastest instant). The hand also races
# on the way back during a wind-up; these were told apart by its direction (back and up).
# The heavy take's overhead blow at 1.15 s goes back and down, and is a blow.
COMBOS = [('light_combo.glb', 'light', [0.22, 0.70, 1.52]),
          ('light_combo_2.glb', 'medium', [0.23, 0.62, 1.32, 2.02]),
          ('heavy_combo.glb', 'great', [0.43, 1.15, 1.80, 2.57])]
# The three jump files are named against what they hold (all three takes are called
# `jump_take_off` inside): take_off pushes and tucks, landing is the hang in the air,
# and in_the_air is the landing, from straight legs down to a crouch and back up.
JUMPS = [('jump_take_off.glb', 'jump'), ('jump_landing.glb', 'jump_fall'), ('jump_in_the_air.glb', 'jump_land')]
# The leap (sandbox `leap.rs`): whole takes out of the Universal Animation Library 2 file,
# picked by name. The game moves the body, so the pelvis' travel goes to `root`, unused.
LEAP = [('flying_forward.glb', 'Flying_Forward', 'leap_glide'),
        ('levitate_idle.glb', 'Levitate_Idle', 'leap_hover'),
        ('UAL2_Standard.glb', 'NinjaJump_Land', 'leap_land'),
        ('UAL2_Standard.glb', 'LayToIdle', 'get_up')]


class Glb:
    def __init__(self, path, animation=None):
        data = path.read_bytes()
        offset, self.bin = 12, b''
        while offset < len(data):
            length, kind = struct.unpack('<II', data[offset:offset+8])
            chunk = data[offset+8:offset+8+length]
            if kind == 0x4E4F534A:
                self.d = json.loads(chunk)
            else:
                self.bin = chunk
            offset += 8 + length
        # Mixamo takes (converted by tools/blender/fbx_to_glb.py) get the mannequin's names.
        for n in self.d['nodes']:
            name = n.get('name', '').split(':')[-1]
            if n.get('name', '').startswith('mixamorig'):
                n['name'] = MIXAMO.get(name, name)
        # Some exports name the head joint in lower case.
        if not any(n.get('name') == 'Head' for n in self.d['nodes']):
            for n in self.d['nodes']:
                if n.get('name') == 'head':
                    n['name'] = 'Head'
        names = [a.get('name') for a in self.d['animations']]
        self.anim = names.index(animation) if animation else 0
        self.nodes = self.d['nodes']
        self.parents = {c: i for i, n in enumerate(self.nodes) for c in n.get('children', [])}
        self.names = {n.get('name'): i for i, n in enumerate(self.nodes)}
        self.order = []
        for i in range(len(self.nodes)):
            self._visit(i)

    def _visit(self, i):
        if i in self.order:
            return
        if i in self.parents:
            self._visit(self.parents[i])
        self.order.append(i)

    def read(self, index):
        a = self.d['accessors'][index]
        v = self.d['bufferViews'][a['bufferView']]
        count = {'SCALAR': 1, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}[a['type']]
        dtype = {5126: '<f4', 5123: '<u2', 5121: 'u1', 5125: '<u4'}[a['componentType']]
        offset = v.get('byteOffset', 0) + a.get('byteOffset', 0)
        return np.frombuffer(self.bin, dtype=dtype, count=a['count']*count, offset=offset).reshape(
            a['count'], count).astype(float)

    def world(self, times):
        """World rotation and position of every node at each time, as (frames, nodes)."""
        anim = self.d['animations'][self.anim]
        n, f = len(self.nodes), len(times)
        rot = np.zeros((f, n, 4))
        pos = np.zeros((f, n, 3))
        scl = np.ones((f, n, 3))
        for i, node in enumerate(self.nodes):
            rot[:, i] = node.get('rotation', [0, 0, 0, 1])
            pos[:, i] = node.get('translation', [0, 0, 0])
            scl[:, i] = node.get('scale', [1, 1, 1])
        for c in anim['channels']:
            sampler = anim['samplers'][c['sampler']]
            t, v = self.read(sampler['input'])[:, 0], self.read(sampler['output'])
            node, path = c['target']['node'], c['target']['path']
            u = np.clip(times, t[0], t[-1])
            if path == 'rotation':
                rot[:, node] = Slerp(t, R.from_quat(v))(u).as_quat() if len(t) > 1 else v[0]
            elif path == 'translation':
                pos[:, node] = np.stack([np.interp(u, t, v[:, k]) for k in range(3)], 1)
            elif path == 'scale':
                scl[:, node] = np.stack([np.interp(u, t, v[:, k]) for k in range(3)], 1)
        wr = [None]*n
        wp = np.zeros((f, n, 3))
        ws = np.ones((f, n))
        for i in self.order:
            local = R.from_quat(rot[:, i])
            if i in self.parents:
                p = self.parents[i]
                wp[:, i] = wp[:, p] + wr[p].apply(pos[:, i] * ws[:, p, None])
                wr[i] = wr[p] * local
                ws[:, i] = ws[:, p] * scl[:, i, 0]
            else:
                wp[:, i], wr[i], ws[:, i] = pos[:, i], local, scl[:, i, 0]
        self.scales = ws
        return wr, wp

    def bind(self):
        """World rotation and position of each joint in the skin's bind pose."""
        skin = self.d['skins'][0]
        ibm = self.read(skin['inverseBindMatrices']).reshape(-1, 4, 4).transpose(0, 2, 1)
        # glTF ignores a skinned mesh's own transform: the bind is the inverse bind matrix
        # as it is (Mixamo exports hang the mesh under a turned centimetre armature).
        mesh_m = np.eye(4)
        out = {}
        for k, j in enumerate(skin['joints']):
            m = mesh_m @ np.linalg.inv(ibm[k])
            u = m[:3, :3] / np.linalg.norm(m[:3, :3], axis=0)
            out[self.nodes[j]['name']] = (R.from_matrix(u), m[:3, 3])
        return out


def frame(direction, pole):
    y = direction / np.linalg.norm(direction)
    z = pole - y*pole.dot(y)
    z /= np.linalg.norm(z)
    x = np.cross(y, z)
    return R.from_matrix(np.stack([x, y, np.cross(x, y)], 1))


def between(a, b):
    a, b = a/np.linalg.norm(a), b/np.linalg.norm(b)
    axis = np.cross(a, b)
    s = np.linalg.norm(axis)
    if s < 1e-9:
        return R.identity()
    return R.from_rotvec(axis/s*np.arctan2(s, a.dot(b)))


class Target:
    """chartest as the glTF stores it: rest = bind, as author_chartest_walk.py assumes."""

    def __init__(self, document):
        sys.path.insert(0, str(ROOT/'tools'))
        from author_chartest_walk import Model
        self.m = Model(document)
        self.joints = [document['nodes'][j]['name'] for j in document['skins'][0]['joints']]

    def rest(self, name):
        i = self.m.names[name]
        return self.m.bind_q[i], self.m.bind_p[i]


_relaxed = {}


def relaxed_clavicles():
    """chartest's clavicles under the chest in the approved `sword_idle` (Mixamo), averaged."""
    if not _relaxed:
        document = json.loads(MODEL.read_text(encoding='utf-8'))
        names = [n.get('name') for n in document['nodes']]
        idle = next(a for a in document['animations'] if a['name'] == 'sword_idle')
        for channel in idle['channels']:
            joint = names[channel['target']['node']]
            if joint in ('clavicle.L', 'clavicle.R') and channel['target']['path'] == 'rotation':
                a = document['accessors'][idle['samplers'][channel['sampler']]['output']]
                view = document['bufferViews'][a['bufferView']]
                data = (MODEL.parent/document['buffers'][view['buffer']]['uri']).read_bytes()
                values = np.frombuffer(data, '<f4', a['count']*4, view.get('byteOffset', 0)+a.get('byteOffset', 0))
                _relaxed[joint] = R.from_quat(values.reshape(-1, 4)).mean()
    return _relaxed


def retarget(target, path, animation=None, relax=False, turn=0.0, pitch=0.0):
    """`turn` (degrees, around up) turns the whole take, not the bind it is measured from.
    `relax` sets the clavicles' average to the relaxed one (see below).
    `pitch` (degrees, around +X, after `turn`) leans the take back, around its mean pelvis."""
    src = Glb(path, animation)
    source_bind = src.bind()
    duration = max(src.read(s['input'])[-1, 0] for s in src.d['animations'][src.anim]['samplers'])
    count = int(round(duration*FPS)) + 1
    times = np.linspace(0, duration, count)
    wr, wp = src.world(times)
    # The mannequin faces +Z; chartest faces -Z.
    flip = R.from_euler('y', 180, degrees=True)
    turned = R.from_euler('y', turn, degrees=True) * flip
    rest_q = {k: flip*q for k, (q, _) in source_bind.items()}
    rest_p = {k: flip.apply(p) for k, (_, p) in source_bind.items()}
    pose_q = {k: turned*wr[src.names[k]] for k in source_bind}
    pose_p = {k: turned.apply(wp[:, src.names[k]]) for k in source_bind}
    if pitch:
        lean = R.from_euler('x', pitch, degrees=True)
        center = pose_p['pelvis'].mean(axis=0)
        pose_q = {k: lean*q for k, q in pose_q.items()}
        pose_p = {k: lean.apply(p-center)+center for k, p in pose_p.items()}
    calibration = {}
    for joint in MAPPING:
        native, _ = target.rest(joint)
        if joint in AIM:
            child, source_child = AIM[joint]
            a = target.rest(child)[1] - target.rest(joint)[1]
            b = rest_p[source_child] - rest_p[MAPPING[joint]]
            calibration[joint] = between(a, b)
        else:
            calibration[joint] = R.identity()
    for side in 'LR':
        s = side.lower()
        hand = target.rest(f'hand.{side}')[1]
        fingers = target.rest(f'fingers.{side}')[1]
        index = target.rest(next(j for j in target.joints if j.startswith(f'index_01_{s}_')))[1]
        if f'ring_01_{s}' not in rest_p:
            # A take on a body without fingers: the hand lines up along the forearm only.
            calibration[f'hand.{side}'] = between(fingers-hand, rest_p[f'hand_{s}']-rest_p[f'lowerarm_{s}'])
            continue
        source_fingers = rest_p[f'middle_01_{s}'] - rest_p[f'hand_{s}']
        source_row = rest_p[f'index_01_{s}'] - rest_p[f'ring_01_{s}']
        calibration[f'hand.{side}'] = frame(source_fingers, source_row) * frame(fingers-hand, index-fingers).inv()
    # Hips height decides the scale of every translation.
    scale = target.rest('hips')[1][1] / rest_p['pelvis'][1]
    world = {}
    for joint, source in MAPPING.items():
        delta = pose_q[source] * rest_q[source].inv()
        world[joint] = delta * calibration[joint] * target.rest(joint)[0]
    m = target.m
    # Mannequin takes shrug: their bind holds the clavicles high, the three.js viewer's
    # exports barely move them off it (levitate, listening, the catch), and even a relaxed
    # UAL idle lands 13 degrees above chartest's. With `relax`, each clavicle keeps only its
    # motion around its own average under the chest, and that average becomes the approved
    # idle's (`relaxed_clavicles`); the upper arm keeps its world rotation, so the arm hangs
    # where the take put it, from a lowered shoulder.
    if relax:
        relaxed = relaxed_clavicles()
        for joint in ('clavicle.L', 'clavicle.R'):
            under = world['chest'].inv() * world[joint]
            world[joint] = world['chest'] * relaxed[joint] * under.mean().inv() * under
    local = {}
    for joint in target.joints:
        i = m.names[joint]
        if joint not in MAPPING or joint == 'root':
            continue
        parent = m.nodes[m.parents[i]]['name']
        parent_world = world.get(parent)
        if parent_world is None:
            parent_world = R.from_quat(np.tile(target.rest(parent)[0].as_quat(), (count, 1)))
        local[joint] = parent_world.inv() * world[joint]
    pelvis = (pose_p['pelvis'] - rest_p['pelvis']) * scale
    hands = np.linalg.norm(pose_p['hand_l'] - pose_p['hand_r'], axis=1) * scale
    speed = np.linalg.norm(np.gradient(pose_p['hand_r'] - pose_p['pelvis'], axis=0), axis=1) * FPS * scale
    feet = np.minimum(pose_p['ball_l'][:, 1], pose_p['ball_r'][:, 1]) * scale
    head = (pose_p['Head'] - pose_p['pelvis']) * scale
    return dict(times=times, local=local, pelvis=pelvis, speed=speed, hands=hands, feet=feet,
                world=world, scale=scale, head=head)


def smooth(values, width=5):
    kernel = np.ones(width)/width
    return np.convolve(np.pad(values, width//2, mode='edge'), kernel, mode='valid')


def cut_combo(take, seconds):
    """One blow per given swing, cut at the hand's slowest point between two of them."""
    speed = smooth(take['speed'], 7)
    peaks = []
    for second in seconds:
        around = int(round(second*FPS))
        lo, hi = max(0, around-6), min(len(speed), around+7)
        peaks.append(lo + int(np.argmax(speed[lo:hi])))
    cuts = [0]
    for a, b in zip(peaks, peaks[1:]):
        cuts.append(a + int(np.argmin(speed[a:b])))
    cuts.append(len(speed)-1)
    return peaks, cuts, speed


def blow_events(peak, start, end, speed, last):
    """Events in the clip's own seconds, from where the hand is fast."""
    fast = speed[peak]*0.45
    on = peak
    while on > start and speed[on-1] > fast:
        on -= 1
    off = peak
    while off < end and speed[off+1] > fast:
        off += 1
    t = lambda f: round((f-start)/FPS, 4)
    # The next blow starts where this one is cut, so it is asked for right at the cut;
    # after the last one, walking away frees the body a little after the follow-through.
    combo = min(off+6, end-2) if last else max(off+1, end-3)
    cancel = min(off+12, end-1) if last else max(combo, end-1)
    events = [(t(max(start, on-6)), 'swing'), (t(on), 'hit_on'), (t(off), 'hit_off'),
              (t(combo), 'combo'), (t(cancel), 'cancel')]
    return [dict(time=float(time), name=name) for time, name in events]


def clip(target, take, start, end, name, events=(), ground=True):
    frames = list(range(start, end+1))
    m = target.m
    rotations = {m.names[j]: q[frames].as_quat() for j, q in take['local'].items()}
    pelvis = take['pelvis']
    # The capsule follows the pelvis' travel smoothed over the whole take; the pelvis keeps
    # only its sway around it. Each blow's root starts at zero, so the body is drawn over
    # the capsule at every cut, wherever the take had walked to by then.
    path = np.stack([smooth(pelvis[:, k], ROOT_SMOOTHING) for k in range(3)], 1)
    path[:, 1] = 0
    if not ground:
        path[:] = 0
    root = path[frames] - path[start]
    hips_world = target.rest('hips')[1] + pelvis[frames] - path[frames]
    root_q, root_p = target.rest('root')
    hips_local = root_q.inv().apply(hips_world - root_p)
    root_track = root_p + root
    return dict(name=name, duration=(end-start)/FPS, rotations=rotations,
                translations={m.names['hips']: hips_local, m.names['root']: root_track},
                events=list(events))


def write(document, clips, buffer=BUFFER, key='chartest_quaternius',
          source='Quaternius Universal Animation Library (CC0), retargeted by tools/import_quaternius_clips.py'):
    generated = document.get('extras', {}).get(key)
    if generated:
        document['animations'] = [a for a in document['animations'] if a['name'] not in set(generated['clips'])]
    sys.path.insert(0, str(ROOT/'tools'))
    from gltf_buffers import drop_buffer
    drop_buffer(document, buffer)
    names = {c['name'] for c in clips}
    replaced = [a['name'] for a in document['animations'] if a['name'] in names]
    document['animations'] = [a for a in document['animations'] if a['name'] not in names]
    data = bytearray()
    buffer_index = len(document['buffers'])

    def accessor(values, kind):
        values = np.ascontiguousarray(values, dtype='<f4')
        view = len(document['bufferViews'])
        document['bufferViews'].append(dict(buffer=buffer_index, byteOffset=len(data), byteLength=values.nbytes))
        data.extend(values.tobytes())
        entry = dict(bufferView=view, componentType=5126, count=len(values), type=kind)
        if kind == 'SCALAR':
            entry.update(min=[float(values.min())], max=[float(values.max())])
        document['accessors'].append(entry)
        return len(document['accessors'])-1

    for c in clips:
        frames = len(next(iter(c['rotations'].values())))
        time = accessor(np.linspace(0, c['duration'], frames), 'SCALAR')
        animation = dict(name=c['name'], channels=[], samplers=[], extras=dict(
            source=source, events=c['events']))
        for kind, tracks in (('rotation', c['rotations']), ('translation', c['translations'])):
            for node, values in tracks.items():
                output = accessor(values, 'VEC4' if kind == 'rotation' else 'VEC3')
                animation['samplers'].append(dict(input=time, output=output, interpolation='LINEAR'))
                animation['channels'].append(dict(sampler=len(animation['samplers'])-1,
                                                  target=dict(node=node, path=kind)))
        document['animations'].append(animation)
    document['buffers'].append(dict(uri=buffer, byteLength=len(data)))
    document.setdefault('extras', {})[key] = dict(clips=sorted(names))
    (MODEL.parent/buffer).write_bytes(data)
    MODEL.write_text(json.dumps(document, indent=1)+'\n', encoding='utf-8')
    return replaced


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--analyze', action='store_true', help='print timings, write nothing')
    args = parser.parse_args()
    document = json.loads(MODEL.read_text(encoding='utf-8'))
    target = Target(document)
    clips = []
    for file, prefix, seconds in COMBOS:
        take = retarget(target, SOURCES/file)
        peaks, cuts, speed = cut_combo(take, seconds)
        print(f'[quaternius] {file}: {len(speed)/FPS:.2f}s, scale {take["scale"]:.3f}, '
              f'travel {np.round(take["pelvis"][-1], 2)}, hands apart {take["hands"].min():.2f}..'
              f'{take["hands"].max():.2f} m')
        for k in range(len(peaks)):
            start, end = cuts[k], cuts[k+1]
            events = blow_events(peaks[k], start, end, speed, k == len(peaks)-1)
            name = f'{prefix}_{k+1}'
            print(f'    {name}: {start/FPS:.2f}..{end/FPS:.2f}s, peak {peaks[k]/FPS:.2f}s '
                  f'{speed[peaks[k]]:.1f} m/s, ' + ', '.join(f'{e["name"]} {e["time"]:.2f}' for e in events))
            clips.append(clip(target, take, start, end, name, events))
    for file, name in JUMPS:
        take = retarget(target, SOURCES/file)
        feet = take['feet'] - take['feet'][0]
        print(f'[quaternius] {file}: {len(feet)/FPS:.2f}s, feet lift max {feet.max():.2f} m at '
              f'{np.argmax(feet)/FPS:.2f}s, hips y {take["pelvis"][:, 1].min():.2f}..{take["pelvis"][:, 1].max():.2f}')
        clips.append((file, name, take))
    for file, animation, name in LEAP:
        take = retarget(target, SOURCES/file, animation, relax=True)
        up = take['world']['chest'].apply([0, 1, 0]) if 'chest' in take['world'] else None
        lean = np.degrees(np.arccos(np.clip(up[:, 1], -1, 1))).mean() if up is not None else 0
        print(f'[quaternius] {animation} -> {name}: {len(take["times"])/FPS:.2f}s, '
              f'travel {np.round(take["pelvis"][-1], 2)}, chest leans {lean:.0f} deg')
        clips.append(clip(target, take, 0, len(take['times'])-1, name))
    if args.analyze:
        return
    jump_clips = []
    for c in clips:
        if isinstance(c, tuple):
            file, name, take = c
            span = JUMP_SPANS.get(name)
            start, end = (int(round(span[0]*FPS)), int(round(span[1]*FPS))) if span else (0, len(take['times'])-1)
            jump_clips.append(clip(target, take, start, end, name, ground=False))
        else:
            jump_clips.append(c)
    replaced = write(document, jump_clips)
    print(f'[quaternius] wrote {len(jump_clips)} clips to {BUFFER}; replaced {replaced}')


# Seconds kept of each jump take: the game starts `jump` on the key, so the crouch
# before take-off is cut, `jump_fall` is held on its last frame (a long fall must not
# replay anything), and `jump_land` ends once the body is back up.
JUMP_SPANS = {'jump': (0.03, 1.0), 'jump_fall': (0.0, 0.8), 'jump_land': (0.0, 0.9)}

if __name__ == '__main__':
    main()
