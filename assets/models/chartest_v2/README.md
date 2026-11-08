# Chartest v2 — customization baseline

Independent copy of the active Chartest as of 2026-11-06. No geometry,
proportions, UVs, weights, or animations have been changed yet. The sandbox
and animator still use the original `../chartest/chartest.gltf`.

- `chartest_v2.blend`: editable copy of the adapted character, including the
  import-time hand and wrist corrections. Internal object and bone names are
  intentionally preserved for animation compatibility.
- `chartest_v2.gltf`, `chartest_v2.bin`, `chartest_v2_walk.bin`: copied runtime
  asset; only buffer filenames in the glTF were changed for this folder.
- `chartest_v2_grip.ron`: runtime hand fitting and free-hand setup.
- `chartest_v2_stow.ron`: copied equipment placement data.
- `chartest_v2_clips/`: copied editable animation clips.
- `Chartest-Grimora-v02-128.png`: independent copy of the current texture.

The runtime grip and free-hand behavior also require Grimora's model loader
and animator; exporting or opening the `.blend` alone does not reproduce the
whole runtime behavior. The adapted `.blend` also predates locomotion added
directly to the glTF; preserve the runtime files and clips during further work.

Do not run the existing import/generation scripts for this copy without
adapting their hardcoded output paths: they currently target `../chartest`.

Customization direction: preserve the body rig and hand chains, refine facial
topology locally, prepare compatible head/ear options, isolate skin coloration,
and validate body proportion changes with clothing, animations and collision.
The current mesh has no morph targets, and the engine currently has no general
support for facial morph deformation. Static exported variants are possible
without that feature.
