"""Convert animation FBX files (Mixamo) into GLB, so the NumPy retargeters can read them.

    blender --background --python tools/blender/fbx_to_glb.py -- <out dir> <file.fbx>...

Each FBX becomes <out dir>/<stem>.glb with its skin and its one take, nothing else.
"""
from pathlib import Path
import sys

import bpy

args = sys.argv[sys.argv.index('--')+1:]
out = Path(args[0])
out.mkdir(parents=True, exist_ok=True)
for path in map(Path, args[1:]):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = 60
    bpy.ops.import_scene.fbx(filepath=str(path))
    target = out / (path.stem.lower().replace(' ', '_').replace('&', 'and').replace('(', '').replace(')', '') + '.glb')
    bpy.ops.export_scene.gltf(filepath=str(target), export_format='GLB', export_animations=True,
                              export_animation_mode='ACTIONS', export_materials='NONE',
                              export_image_format='NONE', export_frame_range=True)
    print(f'[fbx] {path.name} -> {target.name}')
