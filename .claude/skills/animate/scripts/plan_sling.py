"""Search for the musket's draw (`sling_timeline` in tools/gen_stow_clips.py), measuring every frame
the way `tests/gun_clips.rs` does (checked against it): both wrists, how short each arm is, each arm
against the trunk, neck and head, the gun against the body, how fast each elbow moves and how far
the gun slides in the fist at the catch. Run from the repository's root:

    python -I .claude/skills/animate/scripts/plan_sling.py              # search, writes best.json
    python -I .claude/skills/animate/scripts/plan_sling.py show [plan]  # frame by frame

SEED, START (random plans), GENS, STEP (mutation size), INIT (plans to start from, `;` apart) and
OUTPLAN steer the search. Copy the plan it finds into `SLING_PLAN`.
"""
import importlib.util, os, sys, json
import numpy as np

ARGS = sys.argv[1:]
sys.argv = ["x"]
spec = importlib.util.spec_from_file_location("st", os.path.abspath("tools/gen_stow_clips.py"))
st = importlib.util.module_from_spec(spec)
spec.loader.exec_module(st)
gg = st.gun_clips()
# Coarser frames while searching (FPS); `show` and the clips use every frame.
if ARGS[:1] != ["show"]:
    st.STEP = 1.0 / float(os.environ.get("FPS", "20"))
rng = np.random.default_rng(int(os.environ.get("SEED", "1")))


def seg_dist(p, a, b):
    ab = b - a
    u = np.clip((p - a).dot(ab) / ab.dot(ab), 0, 1)
    return np.linalg.norm(p - (a + ab * u))


def body(torso):
    trunk = (np.array((0.0, 0.95, 0.04)), st.chest_moved((0.0, 1.42, 0.05), torso), 0.15)
    neck = (st.chest_moved((0.0, 1.42, 0.05), torso), st.chest_moved((0.0, 1.6, 0.04), torso), 0.075)
    legs = [(np.array((x, 0.92, 0.03)), np.array((x, 0.5, 0.03)), 0.08) for x in (0.1, -0.1)]
    head = st.chest_moved((0.0, 1.66, 0.04), torso)
    return trunk, neck, legs, head


def arm_inside(shoulder, elbow, wrist, trunk, neck, head):
    worst = max(0.0, 0.14 - seg_dist(head, elbow, wrist))
    for a0, b0, r in (trunk, neck):
        for k in np.linspace(0.15, 1.0, 6):
            worst = max(worst, r - 0.01 - seg_dist(elbow + (wrist - elbow) * k, a0, b0))
            if k > 0.5:
                worst = max(worst, r - 0.03 - seg_dist(shoulder + (elbow - shoulder) * k, a0, b0))
    return worst


def evaluate(plan, verbose=False):
    try:
        frames, slide = st.sling_timeline(plan, gg)
    except Exception as e:
        return 1e9, str(e)
    worst = {"slide": (slide, st.SLING_CATCH)}
    short = 0.0
    last = None
    for t, v, gun, turn_l in frames:
        torso = v["torso"]
        lw, ltw, s1, elbow_l = st.left_strain(v["hand_l"], turn_l, torso, v["elbow_l"])
        rw, rtw, s2, elbow_r = st.right_strain(v, torso, v["elbow_r"])
        trunk, neck, legs, head = body(torso)
        pen = 0.0
        if t > st.SLING_GRAB + 1e-6:
            for x in np.arange(-0.4, 1.01, 0.1):
                q = gun.point(x, 0.0, 0.0)
                for a0, b0, r in [trunk] + legs:
                    pen = max(pen, r + 0.02 - seg_dist(q, a0, b0))
                pen = max(pen, 0.14 - np.linalg.norm(q - head))
        wl = np.array(v["hand_l"])
        wr = v["hand_r"]
        arms = max(arm_inside(st.chest_moved((-0.19, 1.453, 0.114), torso), elbow_l, wl, trunk, neck, head),
                   arm_inside(st.chest_moved((0.19, 1.453, 0.114), torso), elbow_r, wr, trunk, neck, head))
        # Behind the back, each hand stays on its own side.
        if wl[2] > -0.05:
            arms = max(arms, (wl[0] + 0.06) * 0.5)
        if wr[2] > -0.05:
            arms = max(arms, (0.06 - wr[0]) * 0.5)
        # The muzzle stays up and the stock low; at the catch the gun is across the front.
        grip, barrel = gun.hand, gun.file_to_body[:, 0]
        shape = max(0.0, grip[1] - 1.35) + max(0.0, 0.35 - barrel[1]) * 0.5
        if abs(t - st.SLING_CATCH) < 1e-6:
            shape += max(0.0, grip[2] + 0.02) + max(0.0, barrel[0] + 0.1) + max(0.0, 0.55 - barrel[1])
        speed = 0.0
        if last is not None:
            dt = t - last[0]
            speed = max(np.linalg.norm(elbow_l - last[1]), np.linalg.norm(elbow_r - last[2])) / dt
        last = (t, elbow_l, elbow_r)
        worst["shape"] = (worst.get("shape", (0, 0))[0] + shape, t)
        for name, val in (("lw", lw), ("ltw", ltw), ("rw", rw), ("rtw", rtw), ("pen", pen), ("arms", arms), ("elbow", speed)):
            if val > worst.get(name, (-1, 0))[0]:
                worst[name] = (val, t)
        short += s1 + s2
        if verbose:
            print(f"{t:.2f} left {lw:4.0f} {ltw:4.0f} right {rw:4.0f} {rtw:4.0f} pen {pen*1000:4.0f} arms {arms*1000:3.0f} short {(s1+s2)*1000:3.0f} "
                  f"elbow {speed:4.1f} hand_r {np.round(wr, 2)} hand_l {np.round(wl, 2)} grip {np.round(grip, 2)} barrel {np.round(barrel, 2)}")
    g = lambda k: float(worst.get(k, (0, 0))[0])
    score = (g("lw") + max(0, g("ltw") - 45) + max(0, g("rw") - 50) + max(0, g("rtw") - 60) + 3000 * short + 3000 * g("pen")
             + 3000 * g("arms") + 300 * g("shape") + 200 * g("slide") + 20 * max(0, g("elbow") - 2.5))
    return score, {k: (round(float(v[0]), 3), v[1]) for k, v in worst.items()}


def unit_rand(near=None, spread=1.0):
    v = rng.normal(size=3) * spread + (np.array(near) if near is not None else 0)
    return tuple(v / np.linalg.norm(v))


def random_plan():
    return {
        "c": rng.uniform(-0.3, 0.05), "alpha": rng.uniform(0, 360), "gamma": rng.uniform(-60, 60), "sigma": float(rng.choice([-1.0, 1.0])),
        "yaw_g": rng.uniform(-40, 5), "pole_g": unit_rand(),
        "reach_mid": tuple(rng.uniform((0.15, 0.8, -0.15), (0.5, 1.25, 0.3))), "pole_mid": unit_rand(),
        "carry_mid": tuple(rng.uniform((0.1, 0.85, -0.4), (0.55, 1.45, 0.25))), "pole_carry": unit_rand(),
        "w_c": tuple(rng.uniform((-0.05, 0.9, -0.4), (0.4, 1.35, -0.05))), "pole_c": unit_rand(),
        "flex_c": tuple(rng.uniform((-50, -50, -25), (50, 50, 25))), "yaw_c": rng.uniform(-30, 15),
        "pole_l": unit_rand((-0.7, -0.8, 0.1), 0.6),
    }


SCALE = {"c": 0.04, "alpha": 20, "gamma": 10, "yaw_g": 4, "yaw_c": 4}


def mutate(plan, k=1.0):
    out = dict(plan)
    for key, v in plan.items():
        if rng.random() > 0.35:
            continue
        if key == "sigma":
            if rng.random() < 0.1:
                out[key] = -v
        elif key in SCALE:
            out[key] = v + rng.normal() * SCALE[key] * k
        elif key.startswith("pole"):
            w = np.array(v) + rng.normal(size=3) * 0.25 * k
            out[key] = tuple(w / np.linalg.norm(w))
        elif key == "flex_c":
            out[key] = tuple(np.array(v) + rng.normal(size=3) * 8 * k)
        else:
            out[key] = tuple(np.array(v) + rng.normal(size=3) * 0.04 * k)
    return out


def as_json(plan):
    return {k: (list(map(float, v)) if np.ndim(v) else float(v)) for k, v in plan.items()}


if __name__ == "__main__":
    if ARGS and ARGS[0] == "show":
        plan = json.load(open(ARGS[1])) if len(ARGS) > 1 else st.SLING_PLAN
        print(evaluate(plan, True))
        sys.exit()
    seeds = [json.load(open(f)) for f in os.environ.get("INIT", "").split(";") if f]
    pop = []
    for i in range(int(os.environ.get("START", "600"))):
        p = (seeds[i] if i < len(seeds) else mutate(seeds[i % len(seeds)], 0.6)) if seeds else random_plan()
        pop.append((evaluate(p)[0], p))
    pop.sort(key=lambda x: x[0])
    step = float(os.environ.get("STEP", "1"))
    for gen in range(int(os.environ.get("GENS", "60"))):
        kids = []
        for score, p in pop[:15]:
            for _ in range(6):
                c = mutate(p, step * (1.0 if gen < 25 else 0.4))
                kids.append((evaluate(c)[0], c))
        pop = sorted(pop[:15] + kids, key=lambda x: x[0])
        print(gen, round(pop[0][0], 1), flush=True)
    best = pop[0][1]
    print(evaluate(best)[1])
    json.dump(as_json(best), open(os.environ.get("OUTPLAN", "best.json"), "w"))
