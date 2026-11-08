"""Author native locomotion, jump and landing clips without regenerating chartest's mesh.

Run from the repository root: python tools/author_chartest_walk.py
Requires numpy and scipy. Axes follow the engine: Y up, -Z forward.
The existing idle supplies the arm/hand pose; this script does not refit the grip.
"""
from copy import deepcopy
import json
from pathlib import Path
import shutil

import numpy as np
from scipy.spatial.transform import Rotation as R, Slerp

ROOT = Path(__file__).resolve().parents[1]
MODEL = ROOT / 'assets/models/chartest/chartest.gltf'
SPEED = 1.2
DURATION = 1.0
FPS = 60
DUTY = 0.60
RUN_SPEED = 5.5
RUN_DURATION = 2/3
RUN_DUTY = .26
JUMP_DURATIONS = {'jump':.30,'jump_fall':.24,'jump_land':.40}
NATIVE_CLIPS = ('sword_walk','sword_run','jump','jump_fall','jump_land','jump_land_walk','jump_land_run')
FORWARD = np.array([0., 0., -1.])


def axis(axis, degrees):
    return R.from_euler(axis, degrees, degrees=True)


def between(a, b):
    a, b = a / np.linalg.norm(a), b / np.linalg.norm(b)
    cross = np.cross(a, b)
    if np.linalg.norm(cross) < 1e-9:
        return R.identity()
    return R.from_rotvec(cross / np.linalg.norm(cross) * np.arctan2(np.linalg.norm(cross), a @ b))


def smooth(u):
    return u*u*u*(10 + u*(-15 + 6*u))


def foot_cycle(phase, rest):
    """Constant-speed support, heel roll, toe push, then a low swing arc."""
    p = phase % 1.
    stroke = SPEED * DURATION * DUTY
    if p < DUTY:
        z = stroke * (p / DUTY - .5)
        if p < .10:
            pitch = 12 * (1 - smooth(p / .10))
        elif p > .40:
            pitch = -27 * smooth((p - .40) / (DUTY - .40))
        else:
            pitch = 0.
        lift = 0.
    else:
        u = (p - DUTY) / (1 - DUTY)
        # Endpoint velocities match the planted foot, rather than pausing it.
        tangent = SPEED * DURATION * (1 - DUTY)
        z = (.5*stroke*(2*u**3-3*u*u+1) - .5*stroke*(-2*u**3+3*u*u)
             + tangent*(2*u**3-3*u*u+u))
        pitch = -27 + 39*smooth(u)
        lift = .085 * np.sin(np.pi*u)**1.7
    position = rest.copy()
    position[0] = np.sign(rest[0]) * .125
    position[2] = .025 + z
    # Rotate about the heel at landing and ball of the foot at push-off.
    pivot = np.array([0., -rest[1], .040 if pitch >= 0 else -.145])
    rotation = axis('x', pitch)
    position += pivot - rotation.apply(pivot)
    position[1] += lift
    return position, rotation, p < DUTY, pitch, lift


class Model:
    def __init__(self, document):
        self.d = document
        self.nodes = document['nodes']
        self.parents = {c:i for i,n in enumerate(self.nodes) for c in n.get('children', [])}
        self.names = {n['name']:i for i,n in enumerate(self.nodes) if 'name' in n}
        self.buffers = [(MODEL.parent / b['uri']).read_bytes() for b in document['buffers']]
        self.local_q = [R.from_quat(n.get('rotation', [0,0,0,1])) for n in self.nodes]
        self.local_p = [np.array(n.get('translation', [0,0,0]), dtype=float) for n in self.nodes]
        self.order = []
        def visit(i):
            if i in self.order: return
            if i in self.parents: visit(self.parents[i])
            self.order.append(i)
        for i in range(len(self.nodes)): visit(i)
        self.bind_p, self.bind_q = self.fk(self.local_p, self.local_q)
        skin = document['skins'][0]
        primitive = next(p for mesh in document['meshes'] for p in mesh['primitives'] if 'JOINTS_0' in p['attributes'])
        attrs = primitive['attributes']
        vertices, joints, weights = self.read(attrs['POSITION']),self.read(attrs['JOINTS_0']).astype(int),self.read(attrs['WEIGHTS_0'])
        weights /= weights.sum(axis=1,keepdims=True)
        self.soles = {}
        for side in ['L','R']:
            foot, toe = [skin['joints'].index(self.names[f'{part}.{side}']) for part in ['foot','toe']]
            fw, tw = (weights*(joints==foot)).sum(axis=1),(weights*(joints==toe)).sum(axis=1)
            mask = (fw+tw>.999)&(vertices[:,1]<.06)
            self.soles[side] = vertices[mask],fw[mask],tw[mask]

    def sole_height(self, side, ankle, rotation, pitch):
        vertices, fw, tw = self.soles[side]
        foot, toe = [self.bind_p[self.names[f'{part}.{side}']] for part in ['foot','toe']]
        posed_foot = ankle+rotation.apply(vertices-foot)
        posed_toe = ankle+rotation.apply(toe-foot)+axis('x',pitch if pitch>=0 else pitch*.18).apply(vertices-toe)
        return ((posed_foot*fw[:,None]+posed_toe*tw[:,None])/(fw+tw)[:,None])[:,1].min()

    def read(self, index):
        a = self.d['accessors'][index]
        v = self.d['bufferViews'][a['bufferView']]
        count = {'SCALAR':1,'VEC3':3,'VEC4':4,'MAT4':16}[a['type']]
        dtype = {5126:'<f4',5123:'<u2',5121:'u1'}[a['componentType']]
        offset = v.get('byteOffset',0) + a.get('byteOffset',0)
        data = np.frombuffer(self.buffers[v['buffer']], dtype=dtype, count=a['count']*count, offset=offset)
        return data.reshape(a['count'],count).copy()

    def fk(self, positions, rotations):
        p, q = {}, {}
        for i in self.order:
            if i in self.parents:
                parent = self.parents[i]
                p[i] = p[parent] + q[parent].apply(positions[i])
                q[i] = q[parent] * rotations[i]
            else:
                p[i], q[i] = positions[i].copy(), rotations[i]
        return p, q

    def sample(self, name, time):
        a = next(a for a in self.d['animations'] if a['name'] == name)
        p, q = deepcopy(self.local_p), list(self.local_q)
        for c in a['channels']:
            sampler = a['samplers'][c['sampler']]
            times, values = self.read(sampler['input'])[:,0], self.read(sampler['output'])
            t = np.clip(time, times[0], times[-1])
            target, kind = c['target']['node'], c['target']['path']
            if kind == 'rotation':
                q[target] = Slerp(times, R.from_quat(values))(t)
            elif kind == 'translation':
                p[target] = np.array([np.interp(t,times,values[:,i]) for i in range(3)])
        return p, q


def run_foot_cycle(phase, rest):
    p = phase % 1.
    stroke = RUN_SPEED*RUN_DURATION*RUN_DUTY
    if p < RUN_DUTY:
        z = stroke*(p/RUN_DUTY-.5)
        pitch = -5*(1-smooth(min(1,p/.065)))
        if p > .09:
            pitch = -36*smooth((p-.09)/(RUN_DUTY-.09))
        lift = 0.
    else:
        u = (p-RUN_DUTY)/(1-RUN_DUTY)
        tangent = RUN_SPEED*RUN_DURATION*(1-RUN_DUTY)
        # Keep the support velocity only close to lift-off/landing. A long
        # Hermite tangent sends the recovering heel beyond this rig's reach.
        # The heel kicks up behind the body first, then the knee drives the foot through and
        # out ahead, and it paws back a little before it lands.
        through = smooth(np.clip((u-.12)/.68,0,1))
        z = stroke*(.5-through) + .22*tangent*u*(1-u)**3 + .06*np.sin(np.pi*np.clip((u-.72)/.28,0,1))
        pitch = keyed(u,[(0,-36),(.3,-24),(.62,9),(.86,4),(1,-5)])
        lift = .42*np.sin(np.pi*np.clip(u/.78,0,1))**1.1*(1-.35*np.clip((u-.3)/.5,0,1))
    position = rest.copy()
    # Runners land almost on one line under the body, not on two rails.
    position[0] = np.sign(rest[0])*.055
    position[2] = .080+z
    pivot = np.array([0.,-rest[1],-.145])
    rotation = axis('x',pitch)
    position += pivot-rotation.apply(pivot)
    position[1] += lift
    return position,rotation,p<RUN_DUTY,pitch,lift


def keyed(time, keys):
    for (a,x),(b,y) in zip(keys,keys[1:]):
        if time <= b:
            return x+(y-x)*smooth(np.clip((time-a)/(b-a),0,1))
    return keys[-1][1]


def jump_foot(motion, time, side, rest):
    if motion=='jump':
        lift = keyed(time,[(0,0),(.18,.36 if side=='R' else .45),(.30,.29 if side=='R' else .36)])
        z = keyed(time,[(0,-.03 if side=='R' else .03),(.18,-.28 if side=='R' else .06),(.30,-.24 if side=='R' else .14)])
        pitch = keyed(time,[(0,-16),(.08,-26 if side=='R' else -35),(.22,6 if side=='R' else -22),(.30,0 if side=='R' else -18)])
    elif motion=='jump_fall':
        lift = keyed(time,[(0,.29 if side=='R' else .36),(.09,.23 if side=='R' else .27),(.24,0 if side=='R' else .04)])
        z = keyed(time,[(0,-.24 if side=='R' else .14),(.24,-.18 if side=='R' else .10)])
        pitch = keyed(time,[(0,0 if side=='R' else -18),(.12,10 if side=='R' else -20),(.24,0 if side=='R' else -10)])
    else:
        lift = keyed(time,[(0,0 if side=='R' else .04),(.045,0)])
        z = -.18 if side=='R' else .10
        pitch = keyed(time,[(0,0 if side=='R' else -10),(.045,0)])
    position = rest.copy()
    position[0] = np.sign(rest[0])*.135
    position[2] = z
    return position,axis('x',pitch),lift<1e-5,pitch,lift


def author(model, running=False, landing=False, motion=None):
    n, bp, bq = model.names, model.bind_p, model.bind_q
    idle_p, idle_q = model.sample('sword_idle', 0.)
    ip, iq = model.fk(idle_p, idle_q)
    duration = RUN_DURATION if running else DURATION
    if motion: duration = JUMP_DURATIONS[motion]
    count = round(duration*FPS)
    elbow_axes = {}
    for side in ['L','R']:
        a,b,c = [ip[n[f'{part}.{side}']] for part in ['upper_arm','forearm','hand']]
        hinge = np.cross(b-a,c-b)
        elbow_axes[side] = hinge/np.linalg.norm(hinge)
    joints = model.d['skins'][0]['joints']
    rotations = {i:[] for i in joints}
    translations = []
    max_reach_error = 0.
    worst_reach = None
    contacts = []
    for k in range(count+1):
        phase = k/count
        time = phase*duration
        w = 2*np.pi*phase
        local_p, local_q = deepcopy(model.local_p), list(model.local_q)
        gp, gq = {}, {}
        desired = {}
        # Lowest in double support, rising over each supporting leg.
        hip_shift = np.array([.018*np.sin(w), -.052-.020*np.cos(2*w), -.020])
        hips_delta = axis('y',4*np.cos(w)) * axis('z',2*np.sin(w)) * axis('x',2)
        torso_delta = axis('y',-4*np.cos(w)) * axis('z',-1.1*np.sin(w)) * axis('x',-1.5)
        if running:
            half = phase % .5
            # Lowest as the leg takes the weight, highest in flight: 7 cm, not a bounce.
            drop = (-.085-.03*np.sin(np.pi*half/RUN_DUTY)**2 if half<RUN_DUTY else
                    -.085+.045*np.sin(np.pi*(half-RUN_DUTY)/(.5-RUN_DUTY))**2)
            hip_shift = np.array([.016*np.sin(w),drop,-.070])
            # The pelvis turns with the swinging leg and drops on its side; the chest turns
            # against it, and dips at each landing.
            hips_delta = axis('y',10*np.cos(w))*axis('z',4*np.sin(w))*axis('x',-9-1.5*np.cos(2*w-.4))
            torso_delta = axis('y',-13*np.cos(w-.3))*axis('z',-3*np.sin(w-.2))*axis('x',-14-3.5*np.cos(2*w-.9))
        if landing:
            absorption = keyed(time,[(0,0),(.09,.105),(.30,0)])
            hip_shift[1] -= absorption
            torso_delta = torso_delta*axis('x',-55*absorption)
        if motion:
            # Pelvis, chest, head and arms react at different times instead of
            # keeping a fixed upper-body pose throughout the ballistic arc.
            if motion=='jump':
                drop = keyed(time,[(0,-.08),(.09,.005),(.22,-.045),(.30,-.04)])
                lean = keyed(time,[(0,-2),(.10,-11),(.22,-7),(.30,-5)])
                pelvis_pitch = keyed(time,[(0,-3),(.08,-7),(.22,4),(.30,2)])
                head_pitch = keyed(time,[(0,0),(.17,-3),(.30,-1)])
            elif motion=='jump_fall':
                drop = keyed(time,[(0,-.04),(.10,-.02),(.24,-.075)])
                lean = keyed(time,[(0,-5),(.10,-1),(.24,-9)])
                pelvis_pitch = keyed(time,[(0,2),(.10,1),(.24,-4)])
                head_pitch = keyed(time,[(0,-1),(.24,-2)])
            else:
                drop = keyed(time,[(0,-.075),(.075,-.22),(.19,-.10),(.40,-.025)])
                lean = keyed(time,[(0,-9),(.115,-19),(.27,-3),(.40,-2)])
                pelvis_pitch = keyed(time,[(0,-4),(.075,-9),(.24,1),(.40,0)])
                head_pitch = keyed(time,[(0,-2),(.15,-7),(.40,0)])
            hip_shift = np.array([0.,drop,-.04])
            hips_delta,torso_delta = axis('x',pelvis_pitch),axis('x',lean)
        desired[n['hips']] = hips_delta*bq[n['hips']]
        desired[n['spine']] = axis('x',-8 if running else -1.5)*bq[n['spine']]
        if running:
            # The spine is the wave between the pelvis and the chest: it twists halfway, bends
            # sideways against the pelvis, and folds a little more at each landing, a beat after it.
            desired[n['spine']] = (axis('y',-2*np.cos(w-.15))*axis('z',-1.5*np.sin(w-.1))
                                   *axis('x',-12-2.5*np.cos(2*w-.6))*bq[n['spine']])
        desired[n['chest']] = torso_delta*bq[n['chest']]
        # Leaning into the run, the head stays level, looking ahead.
        # These are turns in the world: the head stays nearly level and still while the chest works.
        desired[n['neck']] = axis('x',-6 if running else 0)*bq[n['neck']]
        desired[n['head']] = axis('x',-2+1.5*np.cos(2*w-1.2) if running else 0)*bq[n['head']]
        if motion:
            desired[n['spine']] = axis('x',lean*.55)*bq[n['spine']]
            desired[n['neck']] = axis('x',head_pitch*1.4)*bq[n['neck']]
            desired[n['head']] = axis('x',head_pitch)*bq[n['head']]
        for side, sign in [('R',1),('L',-1)]:
            # A small sword-arm swing and a freer opposite arm. The hand keeps
            # exactly its idle rotation relative to the forearm.
            swing = axis('x',-sign*(6 if side == 'R' else 17)*np.cos(w-.15))
            if running:
                swing = axis('x',-sign*(30 if side=='R' else 44)*np.cos(w-.25))
            if motion:
                if motion=='jump':
                    arm = keyed(time,[(0,-8 if side=='R' else -18),(.14,27 if side=='R' else 40),(.30,17 if side=='R' else 28)])
                    flex = keyed(time,[(0,4),(.18,30 if side=='R' else 40),(.30,20 if side=='R' else 28)])
                elif motion=='jump_fall':
                    arm = keyed(time,[(0,17 if side=='R' else 28),(.10,23 if side=='R' else 34),(.24,14 if side=='R' else 20)])
                    flex = keyed(time,[(0,20 if side=='R' else 28),(.24,18 if side=='R' else 28)])
                else:
                    arm = keyed(time,[(0,14 if side=='R' else 20),(.12,6 if side=='R' else 8),(.40,0)])
                    flex = keyed(time,[(0,18 if side=='R' else 28),(.14,24 if side=='R' else 34),(.40,0)])
                swing = axis('x',arm)
            if running:
                # Forward, the arm comes in across the chest; back, it opens a little.
                forward = -sign*np.cos(w-.25)
                swing = swing*axis('z',-sign*(14*max(forward,0)-4*max(-forward,0)))
            arm_delta = swing*torso_delta
            for part in ('clavicle','upper_arm','forearm','hand'):
                i = n[f'{part}.{side}']
                desired[i] = (torso_delta if part=='clavicle' else arm_delta)*iq[i]
                if running and part=='clavicle':
                    # The shoulder goes forward with its arm and back with it.
                    desired[i] = torso_delta*axis('y',sign*7*(-sign*np.cos(w-.25)))*iq[i]
                if running and part in ('forearm','hand'):
                    # Bent near a right angle, more on the way forward than back.
                    flex = (78 if side=='R' else 86)-sign*(15 if side=='R' else 20)*np.cos(w-.25)
                    desired[i] = arm_delta*R.from_rotvec(elbow_axes[side]*np.radians(flex))*iq[i]
                elif motion and part in ('forearm','hand'):
                    desired[i] = arm_delta*R.from_rotvec(elbow_axes[side]*np.radians(flex))*iq[i]
        leg_targets = {}
        for side, offset in [('R',0),('L',.5)]:
            cycle = run_foot_cycle if running else foot_cycle
            ankle, rotation, planted, pitch, lift = cycle(phase+offset,bp[n[f'foot.{side}']])
            if motion:
                ankle,rotation,planted,pitch,lift = jump_foot(motion,time,side,bp[n[f'foot.{side}']])
            ankle[1] += lift-model.sole_height(side,ankle,rotation,pitch)
            leg_targets[side] = (ankle,rotation,planted,pitch)
        for i in model.order:
            parent = model.parents.get(i)
            parent_q = gq[parent] if parent is not None else R.identity()
            if i == n['hips']:
                desired_p = bp[i]+hip_shift
                local_p[i] = parent_q.inv().apply(desired_p-gp[parent])
            if i in desired:
                local_q[i] = parent_q.inv()*desired[i]
            # Preserve all native finger chains and the socket relative to the hand.
            elif i in joints and i not in [n[f'{part}.{s}'] for s in ['L','R'] for part in ['thigh','shin','foot','toe']]:
                if any(part in model.nodes[i].get('name','') for part in ['finger','thumb','index','weapon']):
                    local_q[i] = idle_q[i]
            gp[i] = (gp[parent]+parent_q.apply(local_p[i])) if parent is not None else local_p[i]
            gq[i] = parent_q*local_q[i]
            for side in ['L','R']:
                thigh, shin, foot, toe = [n[f'{part}.{side}'] for part in ['thigh','shin','foot','toe']]
                ankle, rotation, planted, pitch = leg_targets[side]
                if i == thigh:
                    start = gp[i]
                    l1, l2 = np.linalg.norm(bp[shin]-bp[thigh]),np.linalg.norm(bp[foot]-bp[shin])
                    direction = ankle-start
                    distance = np.linalg.norm(direction)
                    excess = distance-(l1+l2)
                    if excess>max_reach_error:
                        max_reach_error = excess
                        worst_reach = (k,side,start.tolist(),ankle.tolist())
                    length = np.clip(distance,abs(l1-l2)+1e-6,l1+l2-1e-6)
                    direction /= distance
                    pole = FORWARD-direction*(FORWARD@direction)
                    pole /= np.linalg.norm(pole)
                    along = (l1*l1-l2*l2+length*length)/(2*length)
                    knee = start+direction*along+pole*np.sqrt(max(0,l1*l1-along*along))
                    desired[thigh] = between(bp[shin]-bp[thigh],knee-start)*bq[thigh]
                    desired[shin] = between(bp[foot]-bp[shin],ankle-knee)*bq[shin]
                    desired[foot] = rotation*bq[foot]
                    # Counter-flex the toe against the ground at push-off.
                    desired[toe] = axis('x',pitch if pitch>=0 else pitch*.18)*bq[toe]
                    local_q[i] = parent_q.inv()*desired[i]
                    gq[i] = desired[i]
                if i == foot:
                    contacts.append(dict(frame=k,side=side,planted=bool(planted),ankle=gp[i].tolist()))
        for i in joints:
            q = local_q[i].as_quat()
            if rotations[i] and np.dot(q,rotations[i][-1]) < 0: q = -q
            rotations[i].append(q)
        translations.append(local_p[n['hips']])
    if max_reach_error > .002:
        raise ValueError(f'Leg target exceeds native reach by {max_reach_error*1000:.1f} mm: {worst_reach}')
    print(f'[{motion or ("land_" if landing else "")+("run" if running else "walk")}] Native reach: max excess {max_reach_error*1000:.3f} mm')
    return rotations,translations,contacts


def main():
    document = json.loads(MODEL.read_text(encoding='utf-8'))
    generated = document.get('extras',{}).get('chartest_walk')
    if generated:
        document['accessors'] = document['accessors'][:generated['accessors']]
        document['bufferViews'] = document['bufferViews'][:generated['views']]
        document['buffers'] = document['buffers'][:generated['buffers']]
        # The original jump lives in the base buffer; replace its channels too.
        document['animations'] = [a for a in document['animations'] if a['name'] not in NATIVE_CLIPS]
    model = Model(document)
    clips = [('sword_walk',DURATION,SPEED,author(model)),
             ('sword_run',RUN_DURATION,RUN_SPEED,author(model,running=True))]
    for motion,duration in JUMP_DURATIONS.items():
        clips.append((motion,duration,0,author(model,motion=motion)))
    clips.extend([
        ('jump_land_walk',DURATION,SPEED,author(model,landing=True)),
        ('jump_land_run',RUN_DURATION,RUN_SPEED,author(model,running=True,landing=True))])
    base = dict(accessors=len(document['accessors']),views=len(document['bufferViews']),buffers=len(document['buffers']))
    data = bytearray()
    buffer_index = len(document['buffers'])
    def accessor(values, kind):
        values = np.array(values,dtype='<f4')
        view = len(document['bufferViews'])
        document['bufferViews'].append(dict(buffer=buffer_index,byteOffset=len(data),byteLength=values.nbytes))
        data.extend(values.tobytes())
        index = len(document['accessors'])
        entry = dict(bufferView=view,componentType=5126,count=len(values),type=kind)
        if kind == 'SCALAR': entry.update(min=[float(values.min())],max=[float(values.max())])
        document['accessors'].append(entry)
        return index
    def channel(node,kind,values):
        output = accessor(values,'VEC4' if kind=='rotation' else 'VEC3')
        sampler = len(animation['samplers'])
        animation['samplers'].append(dict(input=time,output=output,interpolation='LINEAR'))
        animation['channels'].append(dict(sampler=sampler,target=dict(node=node,path=kind)))
    document['animations'] = [a for a in document['animations'] if a['name'] not in NATIVE_CLIPS]
    for name,duration,speed,(rotations,translations,contacts) in clips:
        time = accessor(np.linspace(0,duration,len(translations)),'SCALAR')
        animation = dict(name=name,channels=[],samplers=[],extras=dict(
            source='Grimora native chartest locomotion',speed_mps=speed,
            events=([dict(time=duration/2,name='step')] if name.startswith('jump_land_') else
                    [dict(time=0.,name='step'),dict(time=duration/2,name='step')]) if speed else []))
        for i,values in rotations.items(): channel(i,'rotation',values)
        channel(model.names['hips'],'translation',translations)
        document['animations'].append(animation)
    document['buffers'].append(dict(uri='chartest_walk.bin',byteLength=len(data)))
    document.setdefault('extras',{})['chartest_walk'] = base
    backup = ROOT/'screenshots/walk-review/before'
    backup.mkdir(parents=True,exist_ok=True)
    if not (backup/MODEL.name).exists(): shutil.copy2(MODEL,backup/MODEL.name)
    (MODEL.parent/'chartest_walk.bin').write_bytes(data)
    MODEL.write_text(json.dumps(document,indent=1)+'\n',encoding='utf-8')
    for name in ('sword_walk','sword_run'):
        override = MODEL.parent/f'chartest_clips/{name}.ron'
        if override.exists():
            archive = MODEL.parent/f'authoring_archive/{name}.legacy.ron'
            if not archive.exists(): shutil.copy2(override,archive)
            override.unlink()
    for name,_,_,(_,_,contacts) in clips:
        (ROOT/f'screenshots/walk-review/{name}-contacts.json').write_text(json.dumps(contacts,indent=1)+'\n')
    print(f'[locomotion] Native walk {DURATION:.2f}s at {SPEED:.2f}m/s; run {RUN_DURATION:.3f}s at {RUN_SPEED:.2f}m/s')


if __name__ == '__main__':
    main()
