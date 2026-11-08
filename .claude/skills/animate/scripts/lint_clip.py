#!/usr/bin/env python3
"""Lint PoseClip .ron files for what the engine's tests don't catch.

The solver and the tests in crates/grimora_scene/tests accept clips the user rates 1 out of 10, so
this reads the keys themselves: where the body stops, what never moves, the order things peak in,
and how far each control is asked to go. Thresholds are provisional. Tune them from the user's
grades, in THRESHOLDS below, and note why in references/learned.md.

    python -I lint_clip.py <clip.ron | prefix> [...]       a file, or a clip name prefix
    python -I lint_clip.py --all                           every clip
    python -I lint_clip.py --quiet heavy_                  problems only

A bare name looks in assets/models/chartest/chartest_clips. Exits with 1 if anything FAILs.
"""

import math
import re
import sys
from pathlib import Path

CLIPS = Path(__file__).resolve().parents[4] / "assets/models/chartest/chartest_clips"

THRESHOLDS = {
    "hold_any": 0.12,  # seconds a body control may sit exactly still, outside a blow's active part
    "hips_yaw_blow": 25.0,  # degrees the hips should turn from swing to hit_off
    "torso_yaw_blow": 15.0,
    "contrast": 2.0,  # wind-up seconds per second of cut
    "blade_step": 150.0,  # degrees the blade may turn between two keys
    "edge_flip": 120.0,  # degrees the edge may turn between two keys, where it starts to look inverted
}

# (pitch, yaw, roll) degrees a control may reach from neutral. Provisional, see references/body.md.
RANGES = {
    "torso": ((-25, 50), (-50, 50), (-30, 30)),
    "head": ((-45, 45), (-70, 70), (-35, 35)),
}

WHOLE_BODY = ["root", "hips", "hips_rot", "torso", "head", "foot_l", "foot_r", "hand_r", "blade"]
BODY = ["hips", "hips_rot", "torso"]
ACTIVE = ["hips", "hips_rot", "torso", "hand_r", "blade"]
CHAIN = ["hips_rot", "torso", "hand_r", "blade"]

TRACK = re.compile(r"^\s*(\w+):\s*\[(.*?)^\s*\],", re.S | re.M)
KEY = re.compile(r"\(\s*(-?[\d.]+)\s*,\s*\(\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*,\s*(-?[\d.]+)\s*\)\s*\)")
EVENT = re.compile(r'\(\s*(-?[\d.]+)\s*,\s*"(\w+)"\s*\)')


def parse(path):
    text = path.read_text(encoding="utf-8")
    start = text.index("tracks:")
    end = text.find("events:", start)
    body = text[start + len("tracks:") : end if end >= 0 else len(text)]
    tracks = {}
    for name, keys in TRACK.findall(body):
        found = [(float(k[0]), (float(k[1]), float(k[2]), float(k[3]))) for k in KEY.findall(keys)]
        if found:
            tracks[name] = found
    duration = float(re.search(r"duration:\s*([\d.]+)", text).group(1))
    looping = bool(re.search(r"looping:\s*true", text))
    events = {}
    if end >= 0:
        for time, name in EVENT.findall(text[end:]):
            events.setdefault(name, float(time))
    return {"duration": duration, "looping": looping, "tracks": tracks, "events": events}


def distance(a, b):
    return math.sqrt(sum((x - y) ** 2 for x, y in zip(a, b)))


def angle(a, b):
    la, lb = math.sqrt(sum(x * x for x in a)), math.sqrt(sum(x * x for x in b))
    if la < 1e-6 or lb < 1e-6:
        return 0.0
    cos = sum(x * y for x, y in zip(a, b)) / (la * lb)
    return math.degrees(math.acos(max(-1.0, min(1.0, cos))))


def value_at(keys, t):
    if t <= keys[0][0]:
        return keys[0][1]
    for (t0, v0), (t1, v1) in zip(keys, keys[1:]):
        if t <= t1:
            f = (t - t0) / max(t1 - t0, 1e-6)
            return tuple(a + (b - a) * f for a, b in zip(v0, v1))
    return keys[-1][1]


def holds(keys):
    """(from, to) of each stretch between two keys with the same value."""
    return [(t0, t1) for (t0, v0), (t1, v1) in zip(keys, keys[1:]) if distance(v0, v1) < 1e-4]


def fastest_at(keys, start, end):
    """The middle of the fastest stretch between keys that touches start..end."""
    best = (None, -1.0)
    for (t0, v0), (t1, v1) in zip(keys, keys[1:]):
        if t1 <= start or t0 >= end or t1 - t0 < 1e-4:
            continue
        speed = distance(v0, v1) / (t1 - t0)
        if speed > best[1]:
            best = ((t0 + t1) / 2, speed)
    return best[0]


def lint(clip):
    out = []
    tracks, events = clip["tracks"], clip["events"]
    add = lambda level, text: out.append((level, text))
    blow = "hit_on" in events and "hit_off" in events

    for name, keys in tracks.items():
        if keys[-1][0] > clip["duration"] + 1e-3:
            add("FAIL", f"{name}: a key at {keys[-1][0]:.2f} is past the end ({clip['duration']:.2f})")
        if clip["looping"] and name != "root" and keys[0][0] < 1e-3 and keys[-1][0] > clip["duration"] - 1e-3:
            if distance(keys[0][1], keys[-1][1]) > 1e-3:
                add("FAIL", f"{name}: the loop doesn't close, it jumps {distance(keys[0][1], keys[-1][1]):.3f}")

    for name, limits in RANGES.items():
        for time, value in tracks.get(name, []):
            for axis, (v, (low, high)) in enumerate(zip(value, limits)):
                if not low <= v <= high:
                    label = "pitch yaw roll".split()[axis]
                    add("WARN", f"{name} {label} {v:.0f} at {time:.2f}, outside {low}..{high}")

    for name in ("blade", "edge"):
        limit = THRESHOLDS["blade_step"] if name == "blade" else THRESHOLDS["edge_flip"]
        keys = tracks.get(name, [])
        for (t0, v0), (t1, v1) in zip(keys, keys[1:]):
            turn = angle(v0, v1)
            if turn > limit:
                add("WARN", f"{name} turns {turn:.0f} deg between {t0:.2f} and {t1:.2f}: ambiguous direction")

    if blow:
        swing = events.get("swing", events["hit_on"])
        hit_on, hit_off = events["hit_on"], events["hit_off"]
        # Combo and cancel open in either order, but only after the blade can no longer hit.
        order = [events[n] for n in ("swing", "hit_on", "hit_off") if n in events]
        late = [events[n] for n in ("combo", "cancel") if n in events]
        if order != sorted(order) or any(t < hit_off for t in late):
            add("FAIL", f"events out of order: swing/hit_on/hit_off {order}, combo/cancel {late}")

        missing = [name for name in WHOLE_BODY if name not in tracks]
        if missing:
            add("FAIL", f"never keyed, the body doesn't take part: {', '.join(missing)}")

        for name in ACTIVE:
            for t0, t1 in holds(tracks.get(name, [])):
                overlap = min(t1, hit_off) - max(t0, swing)
                if overlap > 0.02:
                    add("FAIL", f"{name} frozen {t0:.2f}-{t1:.2f}, during the blow ({overlap * 1000:.0f} ms of it)")

        for name, limit in (("hips_rot", "hips_yaw_blow"), ("torso", "torso_yaw_blow")):
            if name in tracks:
                turned = abs(value_at(tracks[name], hit_off)[1] - value_at(tracks[name], swing)[1])
                if turned < THRESHOLDS[limit]:
                    add("FAIL", f"{name} turns only {turned:.0f} deg during the blow (at least {THRESHOLDS[limit]:.0f})")

        wind_up, cut = swing, max(hit_off - swing, 1e-3)
        contrast = wind_up / cut
        if contrast < THRESHOLDS["contrast"]:
            add("WARN", f"wind-up {wind_up:.2f} s for {cut:.2f} s of swing: {contrast:.1f}x, not much contrast")

        peaks = [(n, fastest_at(tracks[n], swing - 0.1, hit_off)) for n in CHAIN if n in tracks]
        peaks = [(n, t) for n, t in peaks if t is not None]
        for (a, ta), (b, tb) in zip(peaks, peaks[1:]):
            if tb < ta - 1e-3:
                add("WARN", f"{b} peaks at {tb:.2f}, before {a} at {ta:.2f}: the chain runs backwards in the keys")
        times = [{k[0] for k in tracks[n]} for n in CHAIN if n in tracks]
        if len(times) >= 2 and all(t == times[0] for t in times):
            add("WARN", "hips, torso, hand and blade share every key time: no overlap between them")

    hold_limit = THRESHOLDS["hold_any"]
    if not clip["looping"]:
        for name in BODY:
            for t0, t1 in holds(tracks.get(name, [])):
                if t1 - t0 > hold_limit and not (blow and t1 > events["hit_on"] and t0 < events["hit_off"]):
                    add("WARN", f"{name} dead still for {t1 - t0:.2f} s ({t0:.2f}-{t1:.2f}): make it a moving hold")
    return out


def main():
    args = sys.argv[1:]
    quiet = "--quiet" in args
    args = [a for a in args if a != "--quiet"]
    paths = []
    if "--all" in args or not args:
        paths = sorted(CLIPS.glob("*.ron"))
    else:
        for arg in args:
            path = Path(arg)
            paths += [path] if path.suffix == ".ron" else sorted(CLIPS.glob(f"{arg}*.ron"))
    if not paths:
        sys.exit(f"no clips match {args}")

    failed = False
    for path in paths:
        clip = parse(path)
        report = lint(clip)
        fails = sum(1 for level, _ in report if level == "FAIL")
        warns = len(report) - fails
        failed |= fails > 0
        if quiet and not report:
            continue
        print(f"{path.stem}: {fails} FAIL, {warns} WARN")
        for level, text in sorted(report, key=lambda item: item[0]):
            print(f"  {level} {text}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
