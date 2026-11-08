import json
import sys

s, repo = sys.argv[1], sys.argv[2]
fit = {c: {int(k): v for k, v in json.load(open(f"{s}/fit_{c}.json")).items() if v} for c in ("heavy_1", "heavy_2", "heavy_3")}


def fmt(d):
    parts = []
    for k, v in d.items():
        if isinstance(v, list):
            parts.append(f'"{k}": (' + ", ".join(f"{round(x, 3):g}" for x in v) + ")")
        else:
            parts.append(f'"{k}": {v:g}')
    return "{" + ", ".join(parts) + "}"


lines = ["FITTED = {"]
for c, beats in fit.items():
    lines.append(f'    "{c}": {{')
    lines += [f"        {i}: {fmt(beats[i])}," for i in sorted(beats)]
    lines.append("    },")
lines.append("}")
p = repo + "/tools/gen_heavy_clips.py"
t = open(p, encoding="utf8").read()
a, b = t.index("# <fitted>\n") + len("# <fitted>\n"), t.index("# </fitted>")
t = t[:a] + "\n".join(lines) + "\n" + t[b:]
open(p, "w", encoding="utf8", newline="\n").write(t)
