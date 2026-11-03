"""The light sword's moveset, written with `anim_kit`: every light sword uses it. Locomotion,
a chain of three one-handed blows, a roll and a backstep. Run through
tools/blender/build_wanderer.py.

The aim is the weight of a souls game at the speed of anime: a short wind-up with a
moment of stillness at its top, an arc that takes a few frames, the whole body landing
behind it, and then a held pose that drifts before the recovery.

Blows chain: each one starts in the pose where the one before settles, and its
`combo` event says when the next may begin. Between `hit_on` and `hit_off` the blade
hurts, and after `cancel` a roll may cut the recovery short. Keep the player's
speeds in step with WALK and RUN.

Where the wrist goes decides whether a pose can be held: a blade at right angles to the
forearm is easy and one in line with it isn't, so keep the `wrist` figure in the report
under about 55. A cut's follow-through must carry the forearm across the body with
the elbow forward, or the blade passes the forearm's line and the hand has to flip.
"""
import math

from anim_kit import Clip, STANDING, keyed, settled, sweep

WALK = 2.4  # meters per second the clips are authored at
RUN = 5.5
STRAFE = 2.2
BACK = 1.9

# Low and loose: the arm hangs with the elbow out, and the tip drops ahead of the feet.
GUARD_HAND = (0.34, 0.86, -0.12)
GUARD_BLADE = (0.26, -0.46, -0.85)
GUARD_EDGE = (0.08, 0.88, -0.46)
GUARD_ELBOW = (0.62, -0.1, 0.78)
GUARD_FEET = {"foot_l": (-0.11, 0.09, -0.10), "foot_r": (0.13, 0.09, 0.14)}
GUARD_FEET_ROT = {"foot_l_rot": (0, 6, 0), "foot_r_rot": (0, -24, 0)}
GUARD = {
    "hips": (0, -0.06, 0.01), "hips_rot": (3, -10, 1), "torso": (5, 12, -1), "head": (2, -2, 0),
    "hand_r": GUARD_HAND, "blade": GUARD_BLADE, "edge": GUARD_EDGE, "elbow_r": GUARD_ELBOW,
    "hand_l": (-0.33, 0.97, -0.05), "elbow_l": (-0.6, -0.1, 0.75),
}
GUARD_SWORD = (GUARD_HAND, GUARD_BLADE, GUARD_EDGE)


def blend(*poses):
    out = {}
    for pose in poses:
        out.update(pose)
    return out


# ---------------------------------------------------------------- walking

def gait(name, speed, period, duty, direction, *, lift=0.11, bob=0.018, lean=0.0, crouch=0.0, roll_foot=1.0,
         hand_r=GUARD_HAND, blade=GUARD_BLADE, edge=GUARD_EDGE, hand_swing=0.05, blade_swing=0.08, arm_swing=0.2, torso_twist=8.0):
    """A walking cycle: each foot stays planted for `duty` of `period` while the root
    travels over it, so nothing slides at the authored speed."""
    stride = speed * duty * period
    dx, dz = direction

    def foot(phase, side, base_x):
        phase %= 1.0
        rock = 0.0
        if phase < duty:
            u = phase / duty
            reach = 0.5 - u
            y = 0.09
            # Rolls onto the toes as the leg trails behind.
            late = max(0.0, u - 0.62) / 0.38
            y += 0.05 * late * late * roll_foot
            rock = 34 * late * late * roll_foot
        else:
            u = (phase - duty) / (1 - duty)
            s = u * u * (3 - 2 * u)
            reach = -0.5 + s
            y = 0.09 + lift * math.sin(math.pi * u) ** 1.3
            rock = (34 * (1 - u) ** 2 - 16 * u * u) * roll_foot
        return (base_x + dx * stride * reach, y, 0.03 + dz * stride * reach), rock

    def sample(t):
        c = dict(STANDING)
        p = t / period
        w = math.tau * p
        c["foot_r"], rock_r = foot(p, "r", 0.085)
        c["foot_l"], rock_l = foot(p + 0.5, "l", -0.085)
        c["foot_r_rot"] = (rock_r, -6, 0)
        c["foot_l_rot"] = (rock_l, 6, 0)
        c["toe_r"], c["toe_l"] = min(rock_r, 30) if rock_r > 0 else 0.0, min(rock_l, 30) if rock_l > 0 else 0.0
        # Lowest just as a foot lands, twice a cycle.
        drop = -crouch - bob * (0.5 + 0.5 * math.cos(2 * w))
        side = 0.018 * math.sin(w)
        c["hips"] = (side, drop, 0)
        c["hips_rot"] = (lean * 0.6, -torso_twist * dz * math.sin(w + 0.4) * -1 if dz else 0.0, 3.0 * math.sin(w))
        c["torso"] = (lean, torso_twist * dz * math.sin(w + 0.4) * -1.4 * -1 if dz else 0.0, -2.0 * math.sin(w))
        c["head"] = (-lean * 0.5, 0, 0)
        swing = math.sin(w + 0.3)
        c["hand_r"] = (hand_r[0], hand_r[1] + drop + 0.06 + hand_swing * 0.4 * math.sin(2 * w - 0.5), hand_r[2] + hand_swing * swing)
        # The tip swings with the arm and dips with each step.
        c["blade"] = (blade[0], blade[1] + blade_swing * 0.5 * math.sin(2 * w - 1.2), blade[2] - blade_swing * swing)
        c["edge"] = edge
        c["elbow_r"] = GUARD_ELBOW
        c["hand_l"] = (-0.3, 1.05 + drop + 0.05 * abs(swing), -0.1 + arm_swing * swing)
        c["elbow_l"] = (-0.4, -0.1, 0.9)
        return c

    return settled(Clip(name, period, sample, [(0.0, "step"), (period / 2, "step")], loop=True))


def idle():
    def sample(t):
        c = dict(STANDING)
        c.update(GUARD)
        c.update(GUARD_FEET)
        c.update(GUARD_FEET_ROT)
        breath = math.sin(math.tau * t / 3.2)
        sway = math.sin(math.tau * t / 3.2 + 1.1)
        hips = GUARD["hips"]
        c["hips"] = (hips[0] + 0.01 * sway, hips[1] - 0.008 * breath, hips[2])
        c["torso"] = (5 - 1.6 * breath, 12 + 1.4 * sway, -1)
        c["head"] = (2 + 1.2 * breath, -2 - 2 * sway, 0)
        x, y, z = GUARD_HAND
        c["hand_r"] = (x + 0.006 * sway, y - 0.01 * breath, z)
        # The tip wanders a little more than the hand does.
        x, y, z = GUARD_BLADE
        c["blade"] = (x + 0.018 * sway, y + 0.02 * breath, z)
        x, y, z = GUARD["hand_l"]
        c["hand_l"] = (x, y - 0.012 * breath, z + 0.008 * sway)
        return c

    return settled(Clip("sword_idle", 3.2, sample, loop=True))


# ---------------------------------------------------------------- blows

# Where the first cut ends, low on the left with the arm across the body.
CUT_END = ((-0.42, 0.97, -0.1), (-0.74, -0.32, -0.59), (0.33, 0.6, -0.73))
CUT_HOLD = ((-0.44, 0.94, -0.08), (-0.75, -0.38, -0.54), (0.3, 0.62, -0.72))
SETTLE_1 = {"hips": (0, -0.17, 0), "hips_rot": (8, 30, -3), "torso": (19, 38, -3), "head": (2, -56, 0),
            "hand_r": CUT_END[0], "blade": CUT_END[1], "edge": CUT_END[2], "elbow_r": (0.2, -0.9, 0.4),
            "hand_l": (-0.42, 1.0, 0.3), "elbow_l": (-0.7, -0.2, 0.3)}
FEET_AFTER_1 = {"foot_l": (-0.11, 0.09, 0.34), "foot_r": (0.15, 0.09, -0.16)}
FEET_ROT_AFTER_1 = {"foot_l_rot": (14, 16, 0), "foot_r_rot": (0, 8, 0)}
# Where the return cut ends, high on the right.
RISE_END = ((0.5, 1.4, -0.22), (0.35, 0.85, 0.38), (-0.05, -0.4, 0.91))
RISE_HOLD = ((0.52, 1.44, -0.2), (0.33, 0.87, 0.36), (-0.05, -0.38, 0.92))
SETTLE_2 = {"hips": (0, -0.1, 0), "hips_rot": (4, -24, 3), "torso": (-3, -28, 2), "head": (-2, 46, 0),
            "hand_r": RISE_END[0], "blade": RISE_END[1], "edge": RISE_END[2], "elbow_r": (0.3, -0.85, 0.4),
            "hand_l": (-0.28, 1.0, -0.36), "elbow_l": (-0.7, -0.3, 0.4)}
FEET_AFTER_2 = {"foot_l": (-0.12, 0.09, -0.10), "foot_r": (0.16, 0.09, 0.25)}
FEET_ROT_AFTER_2 = {"foot_l_rot": (0, 8, 0), "foot_r_rot": (10, -26, 0)}


def step(foot, t0, t1, start, end, lift=0.1, pitch=22):
    """Keys that carry a foot from `start` to `end` in an arc, landing heel first."""
    mid = tuple((a + b) * 0.5 for a, b in zip(start, end))
    rotation = f"{foot}_rot"
    return [
        (t0, {foot: start, rotation: (0, 0, 0)}),
        (t0 + (t1 - t0) * 0.5, {foot: (mid[0], mid[1] + lift, mid[2]), rotation: (pitch * 0.6, 0, 0)}),
        (t1, {foot: end, rotation: (-pitch * 0.7, 0, 0)}),
        (t1 + 0.06, {rotation: (0, 0, 0)}),
    ]


def recover(t0, t1, start, via, elbow_via):
    """The sword coming back to guard, unhurried, and everything else with it."""
    keys = sweep(t0, t1, start, via, GUARD_SWORD, steps=8, late=0.9)
    keys.append((t0 + (t1 - t0) * 0.5, {"elbow_r": elbow_via}))
    keys.append((t1, {k: GUARD[k] for k in ("hips", "hips_rot", "torso", "head", "elbow_r", "hand_l", "elbow_l")}))
    return keys


def sword_1():
    """Up beside the head, a beat of stillness, and a diagonal cut down across the body."""
    start_r, start_l = GUARD_FEET["foot_r"], GUARD_FEET["foot_l"]
    root = -0.45
    planted_r = (FEET_AFTER_1["foot_r"][0], 0.09, FEET_AFTER_1["foot_r"][2] + root)
    top = ((0.40, 1.50, 0.04), (0.3, 0.66, 0.69), (0.15, -0.73, 0.66))
    drift = ((0.42, 1.54, 0.08), (0.25, 0.6, 0.76), (0.13, -0.77, 0.62))
    through = ((0.12, 1.1, -0.54), (-0.15, 0.18, -0.97), (0.4, 0.9, 0.1))
    keys = [
        (0.00, blend(GUARD, GUARD_FEET_ROT, {"root": (0, 0), "foot_r": start_r, "foot_l": start_l})),
        # The wind-up turns the body away and reaches out with the free hand.
        (0.10, {"hips": (0, -0.1, 0.03), "hips_rot": (1, -24, -3), "torso": (-7, -34, -3), "head": (3, 52, 0), "root": (0, -0.02),
                "elbow_r": (0.55, -0.75, 0.35), "hand_l": (-0.3, 1.2, -0.42), "elbow_l": (-0.75, -0.4, 0.4)}),
        (0.15, {"hips": (0, -0.085, 0.04), "hips_rot": (-1, -27, -3), "torso": (-9, -38, -3), "head": (3, 58, 0), "root": (0, -0.05)}),
    ]
    keys += sweep(0.0, 0.11, GUARD_SWORD, ((0.42, 1.18, -0.08), (0.3, 0.75, -0.58), (0.0, 0.6, 0.8)), top, steps=6, late=0.9)
    keys += sweep(0.11, 0.15, top, ((0.41, 1.52, 0.06), (0.28, 0.63, 0.72), (0.14, -0.75, 0.64)), drift, steps=2, late=1.0)
    keys += sweep(0.15, 0.29, drift, through, CUT_END, steps=10, late=1.25)
    keys += [
        # The hips go first and the body lands behind the blade.
        (0.20, {"hips": (0, -0.12, 0.0), "hips_rot": (3, -4, 0), "torso": (6, -6, 0), "head": (5, 8, 0), "root": (0, -0.24), "elbow_r": (0.6, -0.2, 0.3)}),
        (0.29, blend({k: SETTLE_1[k] for k in ("hips", "hips_rot", "torso", "head", "elbow_r")}, {"root": (0, root)})),
        (0.21, {"hand_l": (-0.28, 1.08, -0.1), "elbow_l": (-0.75, -0.2, 0.4)}),
        (0.33, {"hand_l": SETTLE_1["hand_l"], "elbow_l": SETTLE_1["elbow_l"]}),
        # The held pose keeps sinking, so it is never quite still.
        (0.52, {"hips": (0, -0.19, 0), "hips_rot": (9, 32, -3), "torso": (21, 40, -3), "head": (4, -58, 0), "elbow_r": (0.2, -0.9, 0.4),
                "hand_r": CUT_HOLD[0], "blade": CUT_HOLD[1], "edge": CUT_HOLD[2]}),
        (0.52, {"root": (0, root)}),
    ]
    keys += recover(0.54, 0.95, CUT_HOLD, ((-0.02, 0.84, -0.36), (-0.2, -0.55, -0.81), (0.4, 0.75, -0.5)), (0.5, -0.6, 0.6))
    keys += step("foot_r", 0.13, 0.24, start_r, planted_r, lift=0.1)
    # The back foot pivots on its toes as the hips come through.
    keys += [(0.0, {"foot_l": start_l}), (0.62, {"foot_l": start_l}),
             (0.17, {"foot_l_rot": (0, 8, 0)}), (0.27, {"foot_l_rot": FEET_ROT_AFTER_1["foot_l_rot"]}), (0.54, {"foot_l_rot": FEET_ROT_AFTER_1["foot_l_rot"]})]
    end_l = (GUARD_FEET["foot_l"][0], 0.09, GUARD_FEET["foot_l"][2] + root)
    end_r = (GUARD_FEET["foot_r"][0], 0.09, GUARD_FEET["foot_r"][2] + root)
    keys += [(0.70, {"foot_l": ((start_l[0] + end_l[0]) / 2, 0.14, (start_l[2] + end_l[2]) / 2)}), (0.78, {"foot_l": end_l})]
    keys += [(0.70, {"foot_r": planted_r}), (0.80, {"foot_r": ((planted_r[0] + end_r[0]) / 2, 0.13, (planted_r[2] + end_r[2]) / 2)}), (0.90, {"foot_r": end_r})]
    keys += [(0.82, GUARD_FEET_ROT), (0.95, {"root": (0, root)})]
    return keyed("sword_1", 0.95, keys, [(0.15, "swing"), (0.165, "hit_on"), (0.29, "hit_off"), (0.34, "combo"), (0.42, "cancel")])


def sword_2():
    """The blade comes straight back up the line it just cut: the hand doesn't turn over,
    so it's the back edge that leads and nothing is lost between the two."""
    root = -0.4
    start_l, start_r = FEET_AFTER_1["foot_l"], FEET_AFTER_1["foot_r"]
    planted_l = (FEET_AFTER_2["foot_l"][0], 0.09, FEET_AFTER_2["foot_l"][2] + root)
    planted_r = start_r
    dip = ((-0.43, 0.93, -0.09), (-0.72, -0.4, -0.56), (0.32, 0.62, -0.72))
    through = ((0.12, 1.12, -0.52), (-0.02, 0.2, -0.98), (0.4, 0.9, 0.17))
    keys = [
        (0.00, blend(SETTLE_1, FEET_ROT_AFTER_1, {"root": (0, 0), "foot_r": start_r})),
        (0.06, {"hips": (0, -0.19, 0.01), "hips_rot": (8, 28, -3), "torso": (22, 34, -3), "head": (4, -46, 0), "root": (0, -0.02)}),
        (0.12, {"hips": (0, -0.15, -0.01), "hips_rot": (5, 2, 0), "torso": (9, -2, 0), "head": (2, 4, 0), "root": (0, -0.26), "elbow_r": (0.6, -0.2, 0.3)}),
        (0.19, blend({k: SETTLE_2[k] for k in ("hips", "hips_rot", "torso", "head", "elbow_r")}, {"root": (0, root)})),
        (0.13, {"hand_l": (-0.36, 1.02, 0.02), "elbow_l": (-0.7, -0.2, 0.4)}),
        (0.26, {"hand_l": SETTLE_2["hand_l"], "elbow_l": SETTLE_2["elbow_l"]}),
        # Rising onto the front foot and hanging there a moment.
        (0.44, {"hips": (0, -0.075, 0), "hips_rot": (3, -27, 3), "torso": (-5, -31, 2), "head": (-2, 50, 0), "elbow_r": (0.3, -0.85, 0.4),
                "hand_r": RISE_HOLD[0], "blade": RISE_HOLD[1], "edge": RISE_HOLD[2]}),
        (0.44, {"root": (0, root)}),
    ]
    keys += sweep(0.0, 0.06, CUT_END, ((-0.425, 0.95, -0.095), (-0.73, -0.36, -0.575), (0.325, 0.61, -0.725)), dip, steps=3, late=1.0)
    keys += sweep(0.06, 0.18, dip, through, RISE_END, steps=10, late=1.25)
    keys += recover(0.46, 0.9, RISE_HOLD, ((0.46, 1.1, -0.16), (0.4, 0.72, -0.55), (0.0, 0.6, 0.8)), (0.6, -0.6, 0.5))
    keys += step("foot_l", 0.05, 0.17, start_l, planted_l, lift=0.11)
    end_l = (GUARD_FEET["foot_l"][0], 0.09, GUARD_FEET["foot_l"][2] + root)
    end_r = (GUARD_FEET["foot_r"][0], 0.09, GUARD_FEET["foot_r"][2] + root)
    keys += [(0.60, {"foot_l": planted_l}), (0.78, {"foot_l": end_l})]
    keys += [(0.60, {"foot_r": planted_r}), (0.72, {"foot_r": ((planted_r[0] + end_r[0]) / 2, 0.12, (planted_r[2] + end_r[2]) / 2)}), (0.84, {"foot_r": end_r})]
    keys += [(0.0, {"foot_r_rot": FEET_ROT_AFTER_1["foot_r_rot"]}), (0.18, {"foot_r_rot": FEET_ROT_AFTER_2["foot_r_rot"]}), (0.6, {"foot_r_rot": FEET_ROT_AFTER_2["foot_r_rot"]}), (0.84, {"foot_r_rot": GUARD_FEET_ROT["foot_r_rot"]})]
    keys += [(0.9, {"root": (0, root)})]
    return keyed("sword_2", 0.9, keys, [(0.05, "swing"), (0.07, "hit_on"), (0.2, "hit_off"), (0.27, "combo"), (0.38, "cancel")])


def sword_3():
    """The sword drawn back to the hip and the free hand sighting ahead, the body wound up
    side-on and low. Then a long lunge, all of it behind the point. The hand never turns
    over, which is what lets all of this be this fast."""
    root = -1.0
    start_l, start_r = FEET_AFTER_2["foot_l"], FEET_AFTER_2["foot_r"]
    landed_r = (0.14, 0.09, -1.42)
    dragged_l = (-0.13, 0.09, -0.58)
    aim = ((0.38, 1.0, -0.03), (-0.05, 0.02, -1.0), (0.0, 1.0, 0.02))
    aimed = ((0.4, 0.98, 0.02), (-0.06, 0.03, -1.0), (0.0, 1.0, 0.03))
    out = ((0.02, 0.94, -0.78), (-0.02, 0.1, -1.0), (0.0, 1.0, 0.1))
    held = ((0.02, 0.9, -0.76), (-0.02, 0.0, -1.0), (0.0, 1.0, 0.0))
    keys = [
        (0.00, blend(SETTLE_2, FEET_ROT_AFTER_2, {"root": (0, 0), "foot_l": start_l})),
        (0.13, {"hips": (0, -0.2, 0.06), "hips_rot": (2, -32, 0), "torso": (6, -44, 0), "head": (2, 72, 0), "root": (0, 0.03),
                "elbow_r": (0.1, -0.8, 0.55), "hand_l": (-0.1, 1.28, -0.54), "elbow_l": (-0.5, -0.5, 0.5)}),
        (0.2, {"hips": (0, -0.23, 0.08), "hips_rot": (2, -34, 0), "torso": (7, -46, 0), "head": (2, 76, 0), "root": (0, 0.04)}),
        (0.30, {"hips": (0, -0.27, -0.03), "hips_rot": (8, 26, 0), "torso": (20, 30, 0), "head": (-16, -48, 0), "root": (0, root * 0.86), "elbow_r": (0.1, -0.95, 0.3)}),
        (0.36, {"hips": (0, -0.3, -0.02), "hips_rot": (10, 28, 0), "torso": (24, 32, 0), "head": (-20, -52, 0), "root": (0, root)}),
        (0.25, {"hand_l": (-0.3, 1.2, -0.1), "elbow_l": (-0.7, -0.2, 0.3)}),
        (0.4, {"hand_l": (-0.42, 1.02, 0.42), "elbow_l": (-0.7, 0.1, 0.1)}),
        # Held at full stretch; the point sags as the weight comes off it.
        (0.66, {"hips": (0, -0.28, -0.01), "hips_rot": (10, 27, 0), "torso": (22, 30, 0), "head": (-16, -48, 0),
                "hand_r": held[0], "blade": held[1], "edge": held[2]}),
    ]
    keys += sweep(0.0, 0.13, RISE_END, ((0.5, 1.2, -0.1), (0.35, 0.55, -0.76), (0.1, 0.8, 0.6)), aim, steps=6, late=0.9)
    keys += sweep(0.13, 0.2, aim, ((0.39, 0.99, 0.0), (-0.055, 0.025, -1.0), (0.0, 1.0, 0.025)), aimed, steps=2, late=1.0)
    keys += sweep(0.2, 0.29, aimed, ((0.2, 0.96, -0.38), (-0.04, 0.06, -1.0), (0.0, 1.0, 0.06)), out, steps=8, late=1.3)
    keys += recover(0.68, 1.15, held, ((0.15, 0.88, -0.5), (0.15, -0.3, -0.94), (0.05, 0.93, -0.3)), (0.4, -0.7, 0.5))
    keys += step("foot_r", 0.19, 0.29, start_r, landed_r, lift=0.13, pitch=26)
    # The back foot is dragged after the lunge, and then the feet come back under the body.
    keys += [(0.0, {"foot_l": start_l}), (0.24, {"foot_l": start_l}), (0.31, {"foot_l": (-0.12, 0.12, -0.36)}), (0.38, {"foot_l": dragged_l})]
    end_l = (GUARD_FEET["foot_l"][0], 0.09, GUARD_FEET["foot_l"][2] + root)
    end_r = (GUARD_FEET["foot_r"][0], 0.09, GUARD_FEET["foot_r"][2] + root)
    keys += [(0.72, {"foot_l": dragged_l}), (0.82, {"foot_l": (-0.11, 0.14, (dragged_l[2] + end_l[2]) / 2)}), (0.9, {"foot_l": end_l})]
    keys += [(0.84, {"foot_r": landed_r}), (0.97, {"foot_r": (0.14, 0.15, (landed_r[2] + end_r[2]) / 2)}), (1.08, {"foot_r": end_r})]
    keys += [(0.2, {"foot_l_rot": FEET_ROT_AFTER_2["foot_l_rot"]}), (0.3, {"foot_l_rot": (24, 8, 0)}), (0.38, {"foot_l_rot": (0, 10, 0)}), (0.95, {"foot_l_rot": GUARD_FEET_ROT["foot_l_rot"]})]
    keys += [(0.2, {"root": (0, 0.04)}), (1.15, {"root": (0, root)})]
    return keyed("sword_3", 1.15, keys, [(0.2, "swing"), (0.22, "hit_on"), (0.36, "hit_off"), (0.72, "combo"), (0.62, "cancel")])


# ---------------------------------------------------------------- dodges

def roll():
    """A shoulder roll, the body balled up around the hips."""
    end = (0, -3.0)
    keys = [
        (0.00, blend(GUARD, {"root": (0, 0), "tuck": 0.0, "foot_l": (-0.1, 0.09, -0.04), "foot_r": (0.12, 0.09, 0.1), "hand_l": GUARD["hand_l"]})),
        (0.06, {"hips": (0, -0.25, 0), "hips_rot": (22, 0, 0), "torso": (34, 0, 0), "head": (20, 0, 0), "root": (0, -0.34), "tuck": 0.25}),
        (0.14, {"hips": (0, -0.37, 0), "hips_rot": (70, 0, 8), "torso": (62, 0, 0), "head": (34, 0, 0), "root": (0, -0.82), "tuck": 1.0}),
        (0.28, {"hips": (0, -0.33, 0), "hips_rot": (180, 0, 12), "torso": (72, 0, 0), "head": (40, 0, 0), "root": (0, -1.62)}),
        (0.42, {"hips": (0, -0.5, 0), "hips_rot": (296, 0, 6), "torso": (66, 0, 0), "head": (36, 0, 0), "root": (0, -2.34), "tuck": 1.0}),
        (0.56, {"hips": (0, -0.4, 0), "hips_rot": (352, 0, 0), "torso": (42, 0, 0), "head": (20, 0, 0), "root": (0, -2.78), "tuck": 0.32}),
        (0.66, {"hips": (0, -0.22, 0), "hips_rot": (366, -4, 0), "torso": (22, 4, 0), "head": (8, 0, 0), "root": (0, -2.95), "tuck": 0.0}),
        (0.86, blend({k: GUARD[k] for k in ("hips", "torso", "head")}, {"hips_rot": (363, -10, 0), "root": end, "tuck": 0.0})),
    ]
    start_l, start_r = (-0.1, 0.09, -0.04), (0.12, 0.09, 0.1)
    land_l = (-0.11, 0.09, -3.1)
    land_r = (0.13, 0.09, -2.86)
    keys += [(0.13, {"foot_l": start_l, "foot_r": start_r}), (0.46, {"foot_l": land_l, "foot_r": land_r})]
    keys.append((0.0, {"foot_l_rot": (0, 0, 0), "foot_r_rot": (0, 0, 0), "knee_l": (-0.12, 0, -1), "knee_r": (0.12, 0, -1)}))
    keys.append((0.7, GUARD_FEET_ROT))
    return keyed("sword_roll", 0.86, keys, [(0.08, "fall"), (0.5, "fall"), (0.62, "cancel")])


def backstep():
    """A short hop backwards, for when no direction is held."""
    end = (0, 1.55)
    l0, r0 = GUARD_FEET["foot_l"], GUARD_FEET["foot_r"]
    l1 = (l0[0], 0.09, l0[2] + end[1])
    r1 = (r0[0], 0.09, r0[2] + end[1])
    keys = [
        (0.00, blend(GUARD, GUARD_FEET_ROT, {"root": (0, 0)})),
        (0.07, {"hips": (0, -0.14, -0.01), "hips_rot": (9, -8, 0), "torso": (8, 12, 0), "root": (0, 0.1), "hand_r": (0.34, 0.86, -0.17), "hand_l": (-0.3, 0.94, -0.12)}),
        (0.22, {"hips": (0, -0.03, 0.01), "hips_rot": (-5, -8, 0), "torso": (-2, 10, 0), "root": (0, 0.92), "hand_r": (0.35, 0.93, -0.22), "hand_l": (-0.36, 1.05, -0.2)}),
        (0.34, {"hips": (0, -0.17, 0.01), "hips_rot": (9, -10, 0), "torso": (12, 12, 0), "root": (0, 1.48), "hand_r": (0.33, 0.85, -0.13), "hand_l": (-0.3, 0.93, -0.06)}),
        (0.62, blend({k: GUARD[k] for k in ("hips", "hips_rot", "torso", "head", "hand_r", "hand_l")}, {"root": end})),
    ]
    keys += [(0.07, {"foot_l": l0}), (0.2, {"foot_l": (l0[0], 0.2, l0[2] + 0.8)}), (0.31, {"foot_l": l1})]
    keys += [(0.09, {"foot_r": r0}), (0.2, {"foot_r": (r0[0], 0.17, r0[2] + 0.8)}), (0.29, {"foot_r": r1})]
    return keyed("sword_backstep", 0.62, keys, [(0.3, "fall"), (0.4, "cancel")])


def clips():
    run_hand = {"hand_r": (0.35, 0.9, -0.1), "blade": (0.42, -0.74, -0.52), "edge": (0.1, 0.62, -0.78), "hand_swing": 0.11, "blade_swing": 0.14}
    return [
        idle(),
        gait("sword_walk", WALK, 0.74, 0.40, (0, -1), lean=7, crouch=0.045),
        gait("sword_run", RUN, 0.58, 0.27, (0, -1), lift=0.2, bob=0.028, lean=19, crouch=0.08, arm_swing=0.26, torso_twist=12, **run_hand),
        gait("sword_back", BACK, 0.76, 0.42, (0, 1), lift=0.07, bob=0.012, lean=2, crouch=0.04, roll_foot=0.3),
        gait("sword_strafe_l", STRAFE, 0.7, 0.38, (-1, 0), lift=0.07, bob=0.012, lean=3, crouch=0.06, roll_foot=0.25, arm_swing=0.08),
        gait("sword_strafe_r", STRAFE, 0.7, 0.38, (1, 0), lift=0.07, bob=0.012, lean=3, crouch=0.06, roll_foot=0.25, arm_swing=0.08),
        sword_1(),
        sword_2(),
        sword_3(),
        roll(),
        backstep(),
    ]
