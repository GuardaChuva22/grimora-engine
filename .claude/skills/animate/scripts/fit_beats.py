"""Coordinate search over `FITTED` in tools/gen_heavy_clips.py: nudges each beat's blade, edge, poles
and hand, keeping whatever lowers the joints' excess in `tests/moves.rs` and the chain order in
`tests/blows.rs`. Runs the compiled tests directly (under 0.1 s each), and never touches an image.

    python -I fit_beats.py <repo> <moves test .exe> <clip> <passes> [beats to leave alone...]

The state stays in the temp folder, and `bake_fit.py <temp> <repo>` writes it into the generator. A
beat shared by two blows must not change here: keep it out, or copy it to the next blow.
"""
import glob
import importlib.util
import json
import os
import re
import subprocess
import sys
import tempfile

repo, exe, clip, passes = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
skip = {int(x) for x in sys.argv[5:]}
spec = importlib.util.spec_from_file_location("gen", repo + "/tools/gen_heavy_clips.py")
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)
writer = {"heavy_1": gen.write_blow_1, "heavy_2": gen.write_blow_2, "heavy_3": gen.write_blow_3}[clip]
state_file = os.path.join(tempfile.gettempdir(), f"fit_{clip}.json")
fit = {}
if os.path.exists(state_file):
    fit = {int(k): v for k, v in json.load(open(state_file)).items()}
line_re = re.compile(
    r"short\s+(\d+) foot\s+(\d+) arm\s+(\d+)\s+(\d+) \| wrist\s+(\d+)@\S+ twist\s+(\d+)@\S+ left\s+(\d+)@\S+\s+(\d+)@\S+ \| tip low\s+(-?[\d.]+) \| wind-up (\S+) swing (\S+) \((\S+)x\) tip\s*(\S+)/\s*(\S+) m/s \| body\s+(\d+)%.*follow (\S+) s"
)
real_print = print
BLOWS = sorted(glob.glob(repo + "/target/release/deps/blows-*.exe"), key=os.path.getmtime)[-1]
SLACK = 1


def evaluate(f):
    gen.FITTED = {clip: f}
    gen.print = lambda *a, **k: None
    writer()
    env = dict(os.environ, CLIP=clip)
    env.pop("TRACE", None)
    out = subprocess.run([exe, "--nocapture"], env=env, capture_output=True, text=True, cwd=repo + "/crates/grimora_scene").stdout
    m = line_re.search(out)
    if not m:
        raise SystemExit(out[-400:])
    short, foot, ar, al, w, t, wl, tl, low, _, _, _, _, cut, body, follow = [float(x) for x in m.groups()]
    over = lambda v, lim: max(0.0, v - lim)
    blows = subprocess.run([BLOWS, "blows_are", "--nocapture"], env=dict(os.environ, BLOW=clip), capture_output=True, text=True, cwd=repo + "/crates/grimora_scene").stdout
    peaks = [float(x) * 60.0 for x in re.findall(r"\] " + clip + r" (?:\S+\s+fastest\s+\S+ deg/s|tip fastest) at (\S+)", blows)]
    chain = sum(max(0.0, last - peak - SLACK) for last, peak in zip(peaks, peaks[1:]))
    cost = (
        3 * short + 3 * foot + 6 * over(ar, 97) + 6 * over(al, 97)
        + 2 * over(w, 68) + over(t, 98) + 2 * over(wl, 68) + over(tl, 100)
        + 1.5 * over(cut, 40) + 50 * over(0.15, low) + over(40, body) + 200 * over(0.14, follow) + 15 * chain
    )
    size = sum(abs(x) for ch in f.values() for v in ch.values() for x in (v if isinstance(v, list) else [v / 100.0]))
    return cost + 0.5 * size, m.group(0)


beats_count = {"heavy_1": 11, "heavy_2": 10, "heavy_3": 13}[clip]
moves = [("turn", d) for d in (-25, 25, -10, 10)]
for part, step_ in (("blade", 0.2), ("elbow_l", 0.35), ("elbow_r", 0.35), ("hand", 0.04), ("blade", 0.08), ("elbow_l", 0.15), ("elbow_r", 0.15)):
    for axis in range(3):
        for sign in (-1, 1):
            v = [0.0, 0.0, 0.0]
            v[axis] = sign * step_
            moves.append((part, v))

best, text = evaluate(fit)
real_print("start", round(best, 1), flush=True)
for pass_ in range(passes):
    improved = False
    for index in range(1, beats_count - 1):
        if index in skip:
            continue
        for part, delta in moves:
            trial = json.loads(json.dumps(fit))
            trial = {int(k): v for k, v in trial.items()}
            ch = trial.setdefault(index, {})
            if part == "turn":
                ch["turn"] = ch.get("turn", 0) + delta
            else:
                ch[part] = [a + b for a, b in zip(ch.get(part, [0, 0, 0]), delta)]
            cost, line = evaluate(trial)
            if cost < best - 0.05:
                best, text, fit = cost, line, trial
                improved = True
    json.dump(fit, open(state_file, "w"))
    real_print("pass", pass_, round(best, 1), text[:150], flush=True)
    if not improved or best < 1.0:
        break
evaluate(fit)
real_print("final", clip, round(best, 1), text, flush=True)
real_print(json.dumps(fit), flush=True)
