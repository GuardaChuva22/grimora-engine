"""Reusable Grimora Universal Clothing Auto-Fit, Clearance, GLB Export & Preview Kit.

Eliminates boilerplate on all future garments/armors so new pieces only define their mesh + texture
(~100-150 lines) and run in a single ~8s Blender pass.
"""

from collections import defaultdict
import json
import math
import pathlib
import struct

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from build_linen_shirt import (
    CHARTEST_BLEND,
    CHARTEST_GLTF,
    TECIDO_GROSSO_PNG,
    autofit_garment_to_chartest,
    bell,
    build_body_bvh_from_coords,
    clamp,
    enforce_mesh_clearance_over_bvh,
    export_uv_overlay,
    extrude_closed_rim_inward,
    ordered_boundary_loop,
    render_shirt_previews as render_garment_previews,
    set_relaxed_preview_pose,
    smoothstep,
    verify_shirt_and_export_glb as verify_garment_and_export_glb,
)


def open_clean_chartest():
    """Open chartest.blend reset to neutral 1.82m male rest pose and return (body_obj, arm_obj, body_bvh)."""
    bpy.ops.wm.open_mainfile(filepath=str(CHARTEST_BLEND))
    body_obj = bpy.data.objects["chartest_body"]
    arm_obj = bpy.data.objects["chartest_rig"]
    for t in arm_obj.animation_data.nla_tracks:
        t.mute = True
    arm_obj.animation_data.action = None
    arm_obj["height_m"] = 1.82
    arm_obj["is_female"] = False
    arm_obj["breasts_size"] = 0.0
    arm_obj["glutes_size"] = 0.0
    for pb in arm_obj.pose.bones:
        pb.matrix_basis.identity()
    for kb in body_obj.data.shape_keys.key_blocks[1:]:
        kb.value = 0.0
    basis_kb = body_obj.data.shape_keys.key_blocks[0].data
    basis_coords = [basis_kb[i].co.copy() for i in range(len(body_obj.data.vertices))]
    body_bvh, _ = build_body_bvh_from_coords(body_obj.data, basis_coords)
    return body_obj, arm_obj, body_bvh, basis_coords


def clone_clean_body_bmesh(body_obj, basis_coords) -> bmesh.types.BMesh:
    """Create a BMesh from chartest_body Basis with all old UV layers already stripped (prevents UVMap.001 bug)."""
    bm = bmesh.new()
    bm.from_mesh(body_obj.data)
    for old_uv in list(bm.loops.layers.uv):
        bm.loops.layers.uv.remove(old_uv)
    bm.verts.ensure_lookup_table()
    for i, v in enumerate(bm.verts):
        v.co = basis_coords[i].copy()
    return bm


def finalize_garment_pipeline(
    garment_obj,
    body_obj,
    arm_obj,
    tex_img,
    interior_indices: set[int],
    out_dir: pathlib.Path,
    glb_name: str,
    uv_name: str,
    blend_name: str,
):
    """Run UV export -> 42-bone + 24-morph Auto-Fit -> GLB export & 5-body audit -> .blend save -> previews."""
    out_dir.mkdir(parents=True, exist_ok=True)
    garment_obj.parent = arm_obj
    export_uv_overlay(garment_obj, out_dir / uv_name)
    autofit_garment_to_chartest(garment_obj, body_obj, arm_obj, interior_indices)
    verify_garment_and_export_glb(garment_obj, body_obj, arm_obj, tex_img, out_dir / glb_name, interior_indices)
    bpy.ops.wm.save_as_mainfile(filepath=str(out_dir / blend_name))
    render_garment_previews(garment_obj, body_obj, arm_obj, out_dir)
