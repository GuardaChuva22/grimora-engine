"""Writes the katana's moveset: `katana_*`, in assets/models/chartest/chartest_clips.

Same language as tools/gen_heavy_clips.py (whose helpers it uses): a key gives where the arm points
from the chest and how far out (azimuth, elevation, reach), where the blade points (azimuth,
elevation), and the hips, chest and head. Both hands stay on the handle (`grip_l` 1), close
together, as a katana is held.

The katana is not a great sword: the hands stay near the middle line of the body and the blade
is driven by the hips and a step, cutting through and stopping short (no sweep that drags the body
round). The stance is chudan: right foot ahead, left heel behind, the point at an opponent's eyes.

Overwrites those files, so edits made in the animator go away.

    python -I tools/gen_katana_clips.py [clip...]
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
# Left elbow down and in, under the handle, as both hands wring it: found by a search over the
# stance's arm, blade and pole (left wrist 35 degrees, against 104 with the great swords' pole).
os.environ.setdefault("POLE_L", "0.02,0.56,0.04,-0.21")
from gen_heavy_clips import compile_blow, compile_loop, key, step, v  # noqa: E402

# ---- chudan: hands a fist ahead of the navel, the blade forward and up to the eyes of someone in
# front, right hip a little ahead because the right foot is.

CHUDAN = dict(
    hips=(0.01, -0.12, 0.0),
    rot=(4.0, 10.0, 0.0),
    torso=(4.0, -6.0, 0.0),
    head=(0.0, -4.0, 0.0),
    arm=(-18.0, -16.0, 0.96),
    blade=(5.0, 37.0), roll=1.4,
)
FOOT_R = v(0.12, 0.09, -0.24)
FOOT_L = v(-0.14, 0.09, 0.22)
FOOT_R_ROT = v(0, -6, 0)
FOOT_L_ROT = v(0, 22, 0)
# The back heel is off the ground, ready to push.
TOE_L_UP = v(14, 0, 0)

ROOT_1 = 0.32


def write_blow_1():
    """Kesa-giri: the sword goes up over the right shoulder, the right foot steps in, the hips turn
    left and pull the hands down through the middle line, from the right shoulder to the left hip.
    It stops short on the left, and the back foot comes up behind (hikitsuke)."""
    lift = dict(
        hips=(0.02, -0.15, 0.05), rot=(2.0, -22.0, -2.0), torso=(-6.0, -16.0, -4.0), head=(0.0, 36.0, 0.0),
        arm=(12.0, 38.0, 0.70), blade=(150.0, 38.0), roll=-20.0,
    )
    through = dict(
        hips=(-0.01, -0.21, 0.0), rot=(10.0, 12.0, 2.0), torso=(12.0, 6.0, 3.0), head=(-4.0, -16.0, 0.0),
        arm=(-12.0, -14.0, 0.94), blade=(-22.0, 18.0), roll=-6.0,
    )
    stop = dict(
        hips=(-0.03, -0.23, 0.02), rot=(13.0, 30.0, 3.0), torso=(16.0, 14.0, 5.0), head=(-6.0, -36.0, 0.0),
        arm=(-30.0, -26.0, 0.98), blade=(-70.0, -16.0), roll=-12.0,
    )
    keys = [
        key(0.0, CHUDAN),
        # The point rises first, the hands follow it up.
        key(0.1, hips=(0.015, -0.13, 0.03), rot=(3.0, -6.0, -1.0), torso=(-2.0, -8.0, -2.0), head=(0.0, 16.0, 0.0),
            arm=(-4.0, 4.0, 0.76), blade=(40.0, 70.0), roll=-12.0),
        key(0.2, lift),
        # Still loading, not stopped: the hips start back the other way under the raised sword.
        key(0.25, lift, hips=(0.02, -0.17, 0.04), rot=(4.0, -18.0, -1.0), torso=(-7.0, -19.0, -5.0),
            arm=(16.0, 40.0, 0.68), blade=(156.0, 34.0)),
        key(0.3, hips=(0.0, -0.2, 0.01), rot=(8.0, -2.0, 0.0), torso=(4.0, -6.0, 0.0), head=(-2.0, 4.0, 0.0),
            arm=(4.0, 18.0, 0.78), blade=(60.0, 48.0), roll=-14.0),
        key(0.34, through),
        key(0.4, stop),
        # Follow-through: everything keeps going a little, slower, so nothing freezes.
        key(0.56, stop, hips=(-0.03, -0.22, 0.02), rot=(12.0, 33.0, 3.0), torso=(15.0, 16.0, 5.0), head=(-5.0, -38.0, 0.0),
            arm=(-33.0, -29.0, 0.97), blade=(-74.0, -19.0)),
        # mid-return
        key(0.66, hips=(-0.015, -0.2, 0.015), rot=(10.0, 27.0, 2.0), torso=(13.0, 10.0, 3.5), head=(-4.0, -28.0, 0.0), arm=(-30.0, -26.0, 0.97), blade=(-85.0, 0.0), roll=-12.0),
        key(0.76, hips=(0.0, -0.17, 0.01), rot=(8.0, 20.0, 1.0), torso=(10.0, 4.0, 2.0), head=(-2.0, -16.0, 0.0),
            arm=(-31.0, -18.0, 0.93), blade=(-12.0, 39.0), roll=-9.0),
        key(1.1, CHUDAN),
    ]
    foot_r_to = FOOT_R + v(0.0, 0.0, -ROOT_1)
    foot_l_to = FOOT_L + v(0.0, 0.0, -ROOT_1)
    feet = {
        # The front foot steps in as the hips start round, and lands with the cut.
        "foot_r": [(0.0, FOOT_R)] + step(FOOT_R, 0.22, 0.34, foot_r_to, lift=0.05),
        "foot_r_rot": [(0.0, FOOT_R_ROT), (1.1, FOOT_R_ROT)],
        # The back foot pushes, then is pulled up behind the front one.
        "foot_l": [(0.0, FOOT_L), (0.4, FOOT_L)] + step(FOOT_L, 0.42, 0.6, foot_l_to, lift=0.02),
        "foot_l_rot": [(0.0, FOOT_L_ROT), (1.1, FOOT_L_ROT)],
        "toe_l": [(0.0, TOE_L_UP), (0.26, v(26, 0, 0)), (0.36, v(22, 0, 0)), (0.6, TOE_L_UP), (1.1, TOE_L_UP)],
    }
    root = [(0.0, 0.0), (0.2, 0.0), (0.27, -0.06), (0.34, -ROOT_1 * 0.82), (0.6, -ROOT_1)]
    events = [(0.26, "swing"), (0.3, "hit_on"), (0.41, "hit_off"), (0.58, "combo"), (0.66, "cancel")]
    compile_blow("katana_1", 1.1, keys, feet, events, root=root)


# ---- standing, walking and guarding

IDLE_FEET = {
    "foot_l": [(0.0, FOOT_L), (3.2, FOOT_L)],
    "foot_l_rot": [(0.0, FOOT_L_ROT), (3.2, FOOT_L_ROT)],
    "toe_l": [(0.0, TOE_L_UP), (3.2, TOE_L_UP)],
    "foot_r": [(0.0, FOOT_R), (3.2, FOOT_R)],
    "foot_r_rot": [(0.0, FOOT_R_ROT), (3.2, FOOT_R_ROT)],
}
# Over the walk: the same hands, the hips left to the legs.
CARRY = dict(CHUDAN, hips=(0.0, -0.03, 0.0), rot=(0.0, 0.0, 0.0), torso=(4.0, 2.0, 0.0), head=(0.0, -2.0, 0.0))
# The blade raised across the front, point to the left and up, the hands to the right of the face:
# what a blow from ahead meets.
GUARD = dict(
    hips=(0.0, -0.15, 0.01), rot=(4.0, 6.0, 0.0), torso=(4.0, -8.0, 0.0), head=(-2.0, 2.0, 0.0),
    arm=(-12.0, -6.0, 0.64), blade=(-82.0, 26.0),
)


def write_block():
    """The blow lands on the guard and pushes it back and down; it comes back slower."""
    shoved = dict(GUARD, hips=(0.02, -0.19, 0.05), rot=(1.0, 2.0, -2.0), torso=(-6.0, -12.0, -3.0), head=(3.0, 8.0, 0.0),
                  arm=(-6.0, -2.0, 0.56), blade=(-90.0, 34.0), roll=-12.0)
    keys = [
        key(0.0, GUARD),
        key(0.07, shoved),
        key(0.2, shoved, hips=(0.02, -0.2, 0.04), rot=(2.0, 3.0, -1.5), torso=(-4.0, -11.0, -2.5),
            arm=(-7.0, -4.0, 0.57), blade=(-89.0, 32.0)),
        key(0.5, GUARD),
    ]
    compile_blow("katana_block", 0.5, keys, {}, ())


def main(only=()):
    clips = {
        "katana_1": write_blow_1,
        "katana_idle": lambda: compile_loop("katana_idle", 3.2, CHUDAN, breathes=0.8, feet=IDLE_FEET),
        "katana_carry": lambda: compile_loop("katana_carry", 2.0, CARRY, breathes=1.1),
        "katana_guard": lambda: compile_loop("katana_guard", 2.4, GUARD),
        "katana_block": write_block,
    }
    for name, write in clips.items():
        if not only or name in only:
            write()


if __name__ == "__main__":
    main(sys.argv[1:])
