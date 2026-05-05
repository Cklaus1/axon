#!/usr/bin/env bash
# watch.sh — live tail of provenance.jsonl with an updating ASCII chart.
#
# Run this in one terminal; run `axon run examples/asi/optimize.ax` in another.
# Filters to one fn name (default: try_variant) and renders a sparkline of
# scores plus a running min/max/mean.
#
# Usage:
#   ./watch.sh                    # watches try_variant
#   FN=critique_change ./watch.sh # watches a different adaptive fn
#   ./watch.sh --since-now        # ignore historical entries

set -euo pipefail

PROV="${XDG_CACHE_HOME:-$HOME/.cache}/axon/provenance.jsonl"
FN="${FN:-try_variant}"
SINCE_NOW=0
[[ "${1:-}" == "--since-now" ]] && SINCE_NOW=1

mkdir -p "$(dirname "$PROV")"
touch "$PROV"

if [[ "$SINCE_NOW" == "1" ]]; then
    SKIP=$(wc -l <"$PROV")
    tail -n +"$((SKIP + 1))" -F "$PROV"
else
    tail -n 50 -F "$PROV"
fi | python3 - "$FN" <<'PY'
import sys, json

fn_target = sys.argv[1]
scores = []
SPARK = " ▁▂▃▄▅▆▇█"

def render():
    if not scores:
        return ""
    s_min = min(scores)
    s_max = max(scores)
    span = s_max - s_min if s_max > s_min else 1.0
    last = scores[-1]
    ascii = "".join(SPARK[min(8, max(0, int(((s - s_min) / span) * 8)))] for s in scores[-60:])
    mean = sum(scores) / len(scores)
    return (f"\r[{fn_target}] n={len(scores):3d}  "
            f"min={s_min:6.2f}  max={s_max:6.2f}  mean={mean:6.2f}  "
            f"last={last:6.2f}  {ascii}")

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        rec = json.loads(line)
    except json.JSONDecodeError:
        continue
    if rec.get("fn") != fn_target:
        continue
    if rec.get("event") != "return":
        continue
    s = rec.get("score")
    if s is None:
        continue
    scores.append(float(s))
    print(render(), end="", flush=True)
PY
