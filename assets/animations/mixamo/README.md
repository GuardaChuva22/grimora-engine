# Mixamo source motion

`breathing_idle.fbx` was downloaded from the user's authenticated Adobe Mixamo
session on 2026-11-02. Motion: **Breathing Idle**, previewed on the stock Mutant.
Source catalog: https://www.mixamo.com/#/?page=1&query=breathing+idle&type=Motion%2CMotionPack

Download: FBX Binary, **Without Skin**, **60 FPS**, keyframe reduction **none**.
Mixamo controls: Body Type 0, Sway 100, Breathing 100, Overdrive 50,
Character Arm-Space 50, Trim 0–100, Mirror off.

The file contains the original motion and skeleton, no character mesh. Its 596
samples span 9.9167 seconds. Adobe/Mixamo's terms govern the source resource.

`tools/blender/mixamo_idle.py` retargets this motion onto chartest as the embedded
`sword_idle` animation. It calibrates the A/T bind poses, retains native bone lengths,
scales hip displacement, aligns the soles to ground, closes the right hand around
the fitted grip from `hand_grip.py`, and attaches the lowered sword to that hand.
The hand and weapon turn together so the handle cannot rotate out of the palm.
The final 0.4 seconds blend
back to the initial pose for looping. Body joint motion is sampled from this FBX;
it is not reconstructed with the procedural pose solver.
