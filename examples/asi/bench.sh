#!/usr/bin/env bash
# bench.sh — wrap an Axon ASI demo run, capture wall-clock and per-eval data,
# print a structured summary.
#
# Usage:
#   ./bench.sh                  # benches optimize.ax
#   DEMO=classify.ax ./bench.sh # benches classify.ax
#   ./bench.sh --json           # NDJSON output for automation

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEMO_NAME="${DEMO:-optimize.ax}"
DEMO_PATH="$HERE/$DEMO_NAME"
PROV="${XDG_CACHE_HOME:-$HOME/.cache}/axon/provenance.jsonl"
JSON_OUT=0
[[ "${1:-}" == "--json" ]] && JSON_OUT=1

axon_bin() {
    if [[ -x "$REPO/target/release/axon" ]]; then
        echo "$REPO/target/release/axon"
    elif [[ -x "$REPO/target/debug/axon" ]]; then
        echo "$REPO/target/debug/axon"
    else
        echo "ERROR: axon binary not found. Run cargo build -p axon-core first." >&2
        exit 1
    fi
}

# Snapshot: lines in provenance log before this run.
mkdir -p "$(dirname "$PROV")"
touch "$PROV"
PRE_LINES=$(wc -l <"$PROV")

# Time the run; capture exit code without exiting on failure.
T_START=$(date +%s.%N)
set +e
RUN_OUTPUT=$("$(axon_bin)" run "$DEMO_PATH" 2>&1)
RUN_EXIT=$?
set -e
T_END=$(date +%s.%N)

T_WALL=$(python3 -c "print(f'{($T_END) - ($T_START):.3f}')")

# Read the new provenance lines this run produced.
POST_LINES=$(wc -l <"$PROV")
NEW_LINES=$((POST_LINES - PRE_LINES))

# Compute summary in python (no jq; provenance is NDJSON).
SUMMARY=$(tail -n "$NEW_LINES" "$PROV" 2>/dev/null | python3 - <<'PY'
import json, sys, statistics

records = []
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        records.append(json.loads(line))
    except json.JSONDecodeError:
        pass

# Group by fn name.
by_fn = {}
for r in records:
    by_fn.setdefault(r.get("fn", "?"), []).append(r)

print(f"records:{len(records)}")
print(f"distinct_fns:{len(by_fn)}")

for fn, recs in sorted(by_fn.items()):
    scores = [r["score"] for r in recs if "score" in r and r.get("event") == "return"]
    calls  = [r for r in recs if r.get("event") == "call"]
    rets   = [r for r in recs if r.get("event") == "return"]

    print(f"--- fn:{fn}")
    print(f"  call_events:{len(calls)}")
    print(f"  return_events:{len(rets)}")

    if scores:
        print(f"  score_min:{min(scores):.3f}")
        print(f"  score_max:{max(scores):.3f}")
        print(f"  score_mean:{statistics.mean(scores):.3f}")
        print(f"  score_first:{scores[0]:.3f}")
        print(f"  score_last:{scores[-1]:.3f}")
        # Convergence: how many evals to reach 90% of max?
        threshold = 0.9 * max(scores)
        for i, s in enumerate(scores):
            if s >= threshold:
                print(f"  evals_to_90pct_of_max:{i + 1}")
                break

    # Per-eval latency from ts_ms.
    ts = sorted(r.get("ts_ms", 0) for r in recs if r.get("ts_ms"))
    if len(ts) >= 2:
        gaps = [(ts[i] - ts[i-1]) / 1000.0 for i in range(1, len(ts))]
        print(f"  eval_gap_min_s:{min(gaps):.3f}")
        print(f"  eval_gap_max_s:{max(gaps):.3f}")
        print(f"  eval_gap_mean_s:{statistics.mean(gaps):.3f}")
PY
)

if [[ "$JSON_OUT" == "1" ]]; then
    python3 -c "
import json, sys, os
out = {
    'demo': os.environ.get('DEMO_NAME', 'optimize.ax'),
    'wall_clock_s': float(os.environ['T_WALL']),
    'exit_code': int(os.environ['RUN_EXIT']),
    'new_provenance_lines': int(os.environ['NEW_LINES']),
}
for line in os.environ['SUMMARY'].splitlines():
    if ':' in line:
        k, _, v = line.partition(':')
        out[k.strip().lstrip('-').strip()] = v.strip()
print(json.dumps(out))
" 2>/dev/null
else
    cat <<EOF

================================================================
Axon ASI bench report — $DEMO_NAME
================================================================
wall_clock_s     : $T_WALL
exit_code        : $RUN_EXIT
new_prov_lines   : $NEW_LINES
$SUMMARY
================================================================
EOF
    if [[ "$RUN_EXIT" -ne 0 ]]; then
        echo
        echo "--- run output (exit=$RUN_EXIT) ---"
        echo "$RUN_OUTPUT"
    fi
fi
