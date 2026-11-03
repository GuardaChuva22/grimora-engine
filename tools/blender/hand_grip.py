"""Fit a native right hand to the real sword handle without rebuilding its skin.

The result is a constant grip in the model's bind space. Body animation can carry
these rotations with the hand. A bounded, local distal-skin correction clears the
handle's surface; all bones, skin weights and body/palm vertices remain unchanged.
"""
import math

import bpy
import numpy as np
from mathutils import Matrix, Quaternion, Vector

SWORD_SCALE = 0.263


def _closest_triangle_to_axis(points, center, axis, half_length):
    """Exact closest radial point, clipped to the cylinder's axial slab.

    Each polygon vertex carries barycentrics in the original triangle so a
    contact correction can be distributed back to its actual skin vertices.
    """
    polygon = [(points[i] - center, np.eye(3)[i]) for i in range(3)]
    for side in (-1, 1):
        clipped = []
        for i, (point, bary) in enumerate(polygon):
            previous, previous_bary = polygon[i - 1]
            distance = side * np.dot(point, axis) - half_length
            previous_distance = side * np.dot(previous, axis) - half_length
            if (distance <= 0) != (previous_distance <= 0):
                t = previous_distance / (previous_distance - distance)
                clipped.append((previous + t * (point - previous), previous_bary + t * (bary - previous_bary)))
            if distance <= 0:
                clipped.append((point, bary))
        polygon = clipped
        if not polygon:
            return None
    best = None
    for i in range(1, len(polygon) - 1):
        triangle = [polygon[j] for j in (0, i, i + 1)]
        p = np.array([point - axis * np.dot(point, axis) for point, _ in triangle])
        # If the projected triangle covers the axis, radial distance is zero.
        edge_a, edge_b = p[1] - p[0], p[2] - p[0]
        normal = np.cross(edge_a, edge_b)
        determinant = np.dot(normal, normal)
        if determinant > 1e-18:
            u = np.dot(np.cross(-p[0], edge_b), normal) / determinant
            v = np.dot(np.cross(edge_a, -p[0]), normal) / determinant
            if u >= 0 and v >= 0 and u + v <= 1:
                bary = sum(weight * triangle[j][1] for j, weight in enumerate((1 - u - v, u, v)))
                return 0.0, np.zeros(3), bary
        for j in range(3):
            k = (j + 1) % 3
            edge = p[k] - p[j]
            t = float(np.clip(-np.dot(p[j], edge) / max(np.dot(edge, edge), 1e-18), 0, 1))
            radial = p[j] + t * edge
            distance = float(np.linalg.norm(radial))
            if best is None or distance < best[0]:
                best = distance, radial, triangle[j][1] * (1 - t) + triangle[k][1] * t
    return best


def _clear_distal_skin(vertices, weights, hand_weights, triangles, rotations, positions, head,
                       center, axis, radius, half_length):
    """Project only distal skin through its exact inverse LBS map, capped at 6 mm."""
    clearance, limit = 0.0004, 0.006
    eligible = (weights[:, [1, 2, 4, 5, 7, 8]].sum(axis=1) > 0.35) & (hand_weights < 0.15)
    affine = ((1 - weights.sum(axis=1))[:, None, None] * np.eye(3)
              + np.einsum('ni,ijk->njk', weights, rotations))
    offsets = np.einsum('ni,ij->nj', weights, positions - np.einsum('ijk,ik->ij', rotations, head))
    inverse = np.linalg.pinv(affine)
    corrected = vertices.copy()
    posed = np.einsum('nij,nj->ni', affine, corrected) + offsets

    def move(index, displacement):
        candidate = corrected[index] + inverse[index] @ displacement
        delta = candidate - vertices[index]
        length = np.linalg.norm(delta)
        if length > limit:
            candidate = vertices[index] + delta * (limit / length)
        corrected[index] = candidate
        posed[index] = affine[index] @ candidate + offsets[index]

    def deepest():
        vertex_max = 0.0
        for point in posed:
            d = point - center
            if abs(np.dot(d, axis)) <= half_length:
                vertex_max = max(vertex_max, radius - np.linalg.norm(d - axis * np.dot(d, axis)))
        face_max = 0.0
        for triangle in triangles:
            contact = _closest_triangle_to_axis(posed[triangle], center, axis, half_length)
            if contact:
                face_max = max(face_max, radius - contact[0])
        return float(max(vertex_max, 0)), float(max(face_max, 0))

    before = deepest()
    for iteration in range(40):
        for index, point in enumerate(posed):
            if not eligible[index]:
                continue
            d = point - center
            axial = np.dot(d, axis)
            radial = d - axis * axial
            distance = np.linalg.norm(radial)
            if abs(axial) <= half_length and 1e-8 < distance < radius + clearance:
                move(index, radial / distance * (radius + clearance - distance))
        for triangle in triangles:
            contact = _closest_triangle_to_axis(posed[triangle], center, axis, half_length)
            if not contact or contact[0] >= radius + clearance:
                continue
            distance, radial, bary = contact
            movable = eligible[triangle]
            influence = bary * movable
            denominator = float(influence.sum())
            if denominator < 1e-10 or distance < 1e-8:
                continue
            normal = radial / distance
            depth = radius + clearance - distance
            for corner, index in enumerate(triangle):
                if movable[corner]:
                    # A parallel face displacement minimizes the largest corner
                    # motion and avoids an artificial spike at one barycentric corner.
                    move(index, normal * depth / denominator)
        if iteration % 4 == 3 and max(deepest()) < 0.00045:
            break
    after = deepest()
    lengths = np.linalg.norm(corrected - vertices, axis=1)
    worst_triangle = max(triangles, key=lambda tri: radius - (
        _closest_triangle_to_axis(posed[tri], center, axis, half_length) or (radius,))[0])
    return corrected, dict(mesh_correction_vertices=int((lengths > 1e-7).sum()),
        mesh_correction_max_bind_m=float(lengths.max()), mesh_correction_limit_m=limit,
        surface_clearance_target_m=clearance, surface_before_vertex_penetration_m=before[0],
        surface_before_face_penetration_m=before[1], surface_vertex_penetration_m=after[0],
        surface_face_penetration_m=after[1], surface_correction_iterations=iteration + 1,
        surface_worst_triangle_local=worst_triangle.tolist(),
        surface_worst_triangle_eligible=eligible[worst_triangle].tolist(),
        surface_worst_triangle_weights=weights[worst_triangle].tolist(),
        surface_worst_triangle_hand_weights=hand_weights[worst_triangle].tolist())


def _rotation(axis, angle):
    x, y, z = axis
    skew = np.array(((0, -z, y), (z, 0, -x), (-y, x, 0)))
    return np.eye(3) + math.sin(angle) * skew + (1 - math.cos(angle)) * (skew @ skew)


def _handle_dimensions(path):
    """Measure the actual cylindrical mesh in its raw glTF coordinates."""
    previous = set(bpy.context.scene.objects)
    active = bpy.context.view_layer.objects.active
    selected = list(bpy.context.selected_objects)
    bpy.ops.import_scene.gltf(filepath=str(path))
    imported = set(bpy.context.scene.objects) - previous
    try:
        grip = next(o for o in imported if o.type == 'MESH' and 'empunhadura' in o.name.casefold())
        basis = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))
        points = np.array([tuple(basis @ grip.matrix_world @ v.co) for v in grip.data.vertices]) * SWORD_SCALE
        radius = float(np.sqrt(points[:, 0] ** 2 + points[:, 2] ** 2).max())
        half_length = float((points[:, 1].max() - points[:, 1].min()) / 2)
        return radius, half_length
    finally:
        for obj in imported:
            bpy.data.objects.remove(obj, do_unlink=True)
        for obj in selected:
            obj.select_set(True)
        bpy.context.view_layer.objects.active = active


def fit_right_hand(arm, meshes, sword_path):
    """Return global bind-space phalange deltas, socket center and weapon rotation.

    The native index hierarchy must be hand -> index1 -> index2 -> index3.
    All lengths and skin weights remain intact. A deterministic fit uses the skin's
    actual inner pads and tests the entire affected palm against the handle. Only
    distal mesh vertices may then move in bind space, by at most 6 mm, to clear it.
    """
    bones = arm.data.bones
    chains = [
        ['fingers.R', 'fingertips.R', next(b.name for b in bones if b.name.startswith('fingers_03_r_'))],
        [next(b.name for b in bones if b.name.startswith(prefix)) for prefix in
         ('index_01_r_', 'index_02_r_', 'index_03_r_')],
        ['thumb.R', next(b.name for b in bones if b.name.startswith('thumb_02_r_')),
         next(b.name for b in bones if b.name.startswith('thumb_03_r_'))],
    ]
    for chain in chains:
        assert bones[chain[0]].parent.name == 'hand.R', f'{chain[0]} must be parented to hand.R'
        for parent, child in zip(chain, chain[1:]):
            assert bones[child].parent.name == parent, f'{child} must be parented to {parent}'
    names = sum(chains, [])
    index_of = {name: i for i, name in enumerate(names)}
    head = np.array([tuple(bones[name].head_local) for name in names])
    tail = np.array([tuple(bones[name].tail_local) for name in names])
    mcp = head[0].copy()
    finger = np.array(tuple(bones['fingers.R'].head_local - bones['hand.R'].head_local))
    finger /= np.linalg.norm(finger)
    row = head[3] - mcp
    row -= finger * np.dot(row, finger)
    row /= np.linalg.norm(row)
    palm = np.cross(row, finger)
    palm /= np.linalg.norm(palm)
    radius, half_length = _handle_dimensions(sword_path)

    vertices, weights, hand_weights, triangles, vertex_refs = [], [], [], [], []
    for mesh in meshes:
        transform = arm.matrix_world.inverted() @ mesh.matrix_world
        group_names = {g.index: g.name for g in mesh.vertex_groups}
        vertex_map = {}
        for vertex in mesh.data.vertices:
            influences = {group_names[g.group]: g.weight for g in vertex.groups}
            digit = [influences.get(name, 0.0) for name in names]
            hand_weight = influences.get('hand.R', 0.0)
            if sum(digit) > 1e-6 or hand_weight > 0.2:
                vertex_map[vertex.index] = len(vertices)
                vertex_refs.append((mesh, vertex.index, transform.inverted()))
                vertices.append(tuple(transform @ vertex.co))
                weights.append(digit)
                hand_weights.append(hand_weight)
        mesh.data.calc_loop_triangles()
        for triangle in mesh.data.loop_triangles:
            if all(i in vertex_map for i in triangle.vertices):
                triangles.append([vertex_map[i] for i in triangle.vertices])
    vertices, weights = np.array(vertices), np.array(weights)
    hand_weights = np.array(hand_weights)
    triangles = np.array(triangles, dtype=int)
    barycentric = np.array(((1/3, 1/3, 1/3), (0.5, 0.5, 0), (0, 0.5, 0.5), (0.5, 0, 0.5)))
    fixed = vertices * (1 - weights.sum(axis=1))[:, None]
    pad_masks = []
    tip_padding = []
    for chain_index, chain in enumerate(chains):
        ids = list(range(chain_index * 3, chain_index * 3 + 3))
        ownership = weights[:, ids].sum(axis=1)
        pad = np.zeros(len(vertices), dtype=bool)
        thickness = []
        for i in ids:
            segment = tail[i] - head[i]
            t = np.clip(((vertices - head[i]) @ segment) / np.dot(segment, segment), 0, 1)
            offset = vertices - head[i] - t[:, None] * segment
            inward = offset @ (-palm if chain_index == 2 else palm)
            member = weights[:, i] >= 0.3
            pad |= member & (inward > 0.004)
            thickness.extend(inward[member & (inward > 0.004)].tolist())
        if chain_index == 2:
            # Thumb opposition bridges the palm: only its distal pad contacts
            # the handle, not the metacarpal and proximal skin beside it.
            pad &= weights[:, ids[-1]] > 0.3
        pad_masks.append(pad & (ownership > 0.45))
        tip_padding.append(float(np.clip(np.median(thickness) if thickness else 0.01, 0.007, 0.016)))

    # Use the inner surface near the knuckles, excluding the thumb web and wrist.
    relative = vertices - mcp
    palm_selection = (hand_weights > 0.6) & (relative @ finger > -0.05) & (relative @ finger < 0.01)
    palm_surface = float(np.percentile(relative[palm_selection] @ palm, 75)) if palm_selection.any() else 0.01
    preferred_palm = float(np.clip(palm_surface + radius - 0.001, 0.03, 0.05))

    # Socket offsets (palmward, fingerward, along knuckles), then MCP/PIP/DIP
    # flexion for the two finger chains and opposition + three thumb flexions.
    parameters = np.zeros(13)
    parameters[:3] = (preferred_palm, 0.005, 0.01)
    bounds = [(0.028, 0.055), (-0.01, 0.025), (-0.005, 0.025)]
    bounds += [(0, math.radians(v)) for v in (100, 125, 95)] * 2
    bounds += [(math.radians(-10), math.radians(95)),
               (math.radians(-35), math.radians(95)),
               (math.radians(-95), math.radians(110)), (math.radians(-95), math.radians(95))]

    def pose(values):
        rotations = np.empty((9, 3, 3))
        positions = np.empty((9, 3))
        for chain_index in range(3):
            ids = range(chain_index * 3, chain_index * 3 + 3)
            previous_rotation = np.eye(3)
            for segment_index, i in enumerate(ids):
                if chain_index == 2:
                    local = _rotation(row, values[10 + segment_index])
                    if segment_index == 0:
                        local = _rotation(palm, values[9]) @ local
                else:
                    local = _rotation(row, values[3 + chain_index * 3 + segment_index])
                rotations[i] = previous_rotation @ local
                positions[i] = head[i] if segment_index == 0 else (
                    positions[i - 1] + previous_rotation @ (head[i] - head[i - 1]))
                previous_rotation = rotations[i]
        translations = positions - np.einsum('bij,bj->bi', rotations, head)
        deformed = fixed.copy()
        for i in range(9):
            deformed += weights[:, i, None] * (vertices @ rotations[i].T + translations[i])
        tips = positions[[2, 5, 8]] + np.einsum(
            'bij,bj->bi', rotations[[2, 5, 8]], (tail - head)[[2, 5, 8]])
        return rotations, positions, deformed, tips

    def evaluate(values, details=False):
        center = mcp + palm * values[0] + finger * values[1] + row * values[2]
        rotations, positions, deformed, tips = pose(values)
        surface = np.einsum('sk,tkd->tsd', barycentric, deformed[triangles]).reshape(-1, 3)
        q_surface = surface - center
        surface_axial = q_surface @ row
        surface_radial = np.linalg.norm(q_surface - surface_axial[:, None] * row, axis=1)
        face_penetration = np.maximum(radius - surface_radial - 0.001, 0) * (
            np.abs(surface_axial) < half_length - 0.002)
        q = deformed - center
        axial = q @ row
        radial = np.linalg.norm(q - axial[:, None] * row, axis=1)
        within = np.abs(axial) < half_length - 0.002
        penetration = np.maximum(radius - radial - 0.001, 0) * within
        deepest = max(penetration.max(), face_penetration.max())
        loss = float(np.mean(penetration ** 2) * 300 + np.mean(face_penetration ** 2) * 300 + deepest ** 2 * 500)
        # Inner pads should touch, while skin penetration is much more expensive.
        contact_errors = []
        for chain_index in range(3):
            pad = pad_masks[chain_index] & within
            errors = radial[pad] - radius
            contact_errors.append(float(np.sqrt(np.mean(errors ** 2))) if errors.size else 0.0)
            if errors.size:
                loss += float(np.mean(errors ** 2) * 3)
            distal = pad & (weights[:, chain_index * 3 + 2] > 0.3)
            if distal.any():
                loss += float(np.mean((radial[distal] - radius) ** 2) * 35)
            rc = radius + tip_padding[chain_index]
            base = head[chain_index * 3]
            if chain_index < 2:
                start = base - center
                start -= row * np.dot(start, row)
                start = start / np.linalg.norm(start) * rc
                length = sum(np.linalg.norm(tail[i] - head[i]) for i in range(chain_index * 3, chain_index * 3 + 3))
                sweep = min(length / rc, math.pi * 0.88)
                target = center + row * np.dot(base - center, row) + _rotation(row, sweep) @ start
            else:
                # The opposed thumb closes over the index side of the cylinder.
                target = center + row * 0.031 + palm * rc
            loss += float(np.sum((tips[chain_index] - target) ** 2) * 8)
        loss += float((values[0] - preferred_palm) ** 2 * 8 + values[1] ** 2 * 0.3)
        if details:
            return dict(loss=loss, radius_m=radius, half_length_m=half_length,
                        max_penetration_m=float(penetration.max()),
                        max_face_penetration_m=float(face_penetration.max()),
                        face_samples=int(len(face_penetration)),
                        penetration_vertices=int((penetration > 0.002).sum()),
                        pad_rms_gap_m=contact_errors, tip_padding_m=tip_padding,
                        center_offsets_m=values[:3].tolist(),
                        curls_degrees=np.degrees(values[3:]).tolist()), rotations, deformed, center
        return loss

    # Initialize from a chordal approximation to the measured cylinder, not fixed
    # animation angles. Optimization then accounts for the real mesh's pads.
    for chain_index in range(2):
        rc = radius + tip_padding[chain_index]
        previous = 0.0
        previous_delta = 0.0
        for segment_index in range(3):
            i = chain_index * 3 + segment_index
            chord = np.linalg.norm(tail[i] - head[i])
            bend = 2 * math.asin(min(0.95, chord / (2 * rc)))
            absolute = previous + bend / 2
            rest_direction = tail[i] - head[i]
            rest_angle = math.atan2(float(np.dot(rest_direction, palm)), float(np.dot(rest_direction, finger)))
            global_delta = absolute - rest_angle
            parameters[3 + i] = global_delta - previous_delta
            previous_delta = global_delta
            previous += bend
    parameters[9] = math.radians(25)
    parameters[10:] = math.radians(10)
    parameters = np.array([np.clip(v, *bound) for v, bound in zip(parameters, bounds)])
    initial_loss = evaluate(parameters)
    seeds = [parameters.copy()]
    for spread in (0.006, 0.012):
        seed = parameters.copy()
        seed[0] = min(bounds[0][1], seed[0] + spread)
        seeds.append(seed)
    best_parameters = parameters.copy()
    best_loss = initial_loss
    for parameters in seeds:
        for degrees, millimeters, rounds in [(18, 4, 5), (9, 2, 5), (4, 1, 5), (1.5, 0.4, 5), (0.5, 0.15, 3)]:
            steps = [millimeters / 1000] * 3 + [math.radians(degrees)] * 10
            for _ in range(rounds):
                improved = False
                for i, step in enumerate(steps):
                    best = evaluate(parameters)
                    best_value = parameters[i]
                    for sign in (-1, 1):
                        trial = parameters.copy()
                        trial[i] = np.clip(parameters[i] + sign * step, *bounds[i])
                        score = evaluate(trial)
                        if score < best:
                            best, best_value = score, trial[i]
                    if best_value != parameters[i]:
                        improved = True
                        parameters[i] = best_value
                if not improved:
                    break
        score = evaluate(parameters)
        if score < best_loss:
            best_loss, best_parameters = score, parameters.copy()
    parameters = best_parameters
    metrics, rotations, deformed, center = evaluate(parameters, details=True)
    metrics['initial_loss'] = initial_loss
    metrics['pad_vertices'] = [int(x.sum()) for x in pad_masks]
    _, positions, _, _ = pose(parameters)
    corrected, surface_metrics = _clear_distal_skin(vertices, weights, hand_weights, triangles,
        rotations, positions, head, center, row, radius, half_length)
    metrics.update(surface_metrics)
    for index, (mesh, vertex_index, inverse_transform) in enumerate(vertex_refs):
        if np.linalg.norm(corrected[index] - vertices[index]) > 1e-7:
            mesh.data.vertices[vertex_index].co = inverse_transform @ Vector(tuple(corrected[index]))
    for mesh in meshes:
        mesh.data.update()
    # Local Y is the sword's axial direction; keep its guard in the palm plane.
    # The blade exits the index/thumb end, never the little-finger end.
    y, z = Vector(tuple(row)), Vector(tuple(palm))
    x = y.cross(z).normalized()
    weapon_rotation = Matrix((x, y, x.cross(y))).transposed().to_quaternion()
    print('[hand-grip]', metrics)
    return dict(deltas={name: Matrix(rotations[i].tolist()).to_quaternion() for i, name in enumerate(names)},
                center=Vector(tuple(center)), weapon_rotation=weapon_rotation, metrics=metrics)
