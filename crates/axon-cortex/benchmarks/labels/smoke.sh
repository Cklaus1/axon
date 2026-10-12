#!/usr/bin/env bash
# benchmarks/labels/smoke.sh — `cortex repair` with CORTEX_LABEL_LOG on temp
# copies of examples/cortex/pricing.ax, then `to_items.py convert` on the log.
#
# Three runs, no model, $0: a verified repair with --label-source (becomes an
# item), a wrong body (no truth: skipped with that reason), and a verified
# repair without --label-source (no source: skipped with that reason).
#
# Binaries: $AXON and $CORTEX_BIN when named, else the ones this script builds
# (scripts/lib/axon_bin.sh). Exits non-zero if any exit code or count differs.
set -u
REPO="$(cd "$(dirname "$0")/../../../.." && pwd)"
cd "$REPO" || exit 2
. "$REPO/scripts/lib/axon_bin.sh"

if [ -n "${AXON:-}" ]; then
  named_bin AXON labels-smoke
else
  cargo build -q -p axon-core --no-default-features --bin axon || exit 2
  use_built AXON axon
fi
if [ -n "${CORTEX_BIN:-}" ]; then
  named_bin CORTEX_BIN labels-smoke
else
  cargo build -q -p axon-cortex --bin cortex || exit 2
  use_built CORTEX_BIN cortex
fi
export CORTEX_RANKER=spectrum CORTEX_AUDIT=off

SRC="$REPO/examples/cortex/pricing.ax"
FIX='if total >= 100 { total / 10 } else { 0 }'
WRONG='if total > 0 { total / 10 } else { 0 }'
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
export CORTEX_LABEL_LOG="$WORK/labels.jsonl"
fails=0

# run WANT NAME ARGS... — one repair on a fresh copy of pricing.ax.
run() {
  local want="$1" name="$2"
  shift 2
  cp "$SRC" "$WORK/pricing.ax"
  echo
  echo "== $name"
  "$CORTEX_BIN" repair --workspace "$WORK" --file pricing.ax \
    --check hidden_discount_boundary --axon "$AXON" --write-prefix pricing.ax "$@"
  local got=$?
  if [ "$got" = "$want" ]; then
    echo "-- expected exit $want, got $got: ok"
  else
    echo "-- expected exit $want, got $got: MISMATCH"
    fails=$((fails + 1))
  fi
}

run 0 "verified, --label-source" --generator "literal:$FIX" --label-source
run 21 "wrong body, --label-source" --generator "literal:$WRONG" --label-source
run 0 "verified, no source" --generator "literal:$FIX"

echo
echo "== $CORTEX_LABEL_LOG: $(wc -l < "$CORTEX_LABEL_LOG") line(s), mode $(stat -c %a "$CORTEX_LABEL_LOG")"
python3 - "$CORTEX_LABEL_LOG" <<'EOF'
import json, sys
for line in open(sys.argv[1]):
    r = json.loads(line)
    print(json.dumps({k: r[k] for k in ("outcome", "exit_code", "truth", "attempted", "proposals")}
                     | {"source": None if r["source"] is None else f"<{len(r['source'])} chars>"}))
EOF

echo
echo "== to_items.py convert"
python3 "$REPO/crates/axon-cortex/benchmarks/labels/to_items.py" convert \
  "$CORTEX_LABEL_LOG" --out "$WORK/items" || fails=$((fails + 1))
python3 - "$WORK/items" <<'EOF' || fails=$((fails + 1))
import json, os, sys
d = sys.argv[1]
s = json.load(open(os.path.join(d, "summary.json")))
lr = [json.loads(l) for l in open(os.path.join(d, "locrank_items.jsonl"))]
dl = [json.loads(l) for l in open(os.path.join(d, "decision_loc.jsonl"))]
for x in lr:
    print("locrank item:", x["id"], "truth", x["loc"]["truth"], "spectrum_rank",
          x["loc"]["spectrum_rank"], "eligible", x["loc"]["eligible"])
for x in dl:
    print("decision item:", x["id"], "label", x["label"], "criteria", x["question"]["criteria"])
want = {"no source: the run did not pass --label-source": 1, "no truth: outcome no_progress": 1}
ok = (s["rows"], len(lr), len(dl)) == (3, 1, 1) and all(v == want for v in s["skipped"].values())
print("-- 3 rows -> 1 locrank item, 1 decision item, 2 skips with reasons:", "ok" if ok else "MISMATCH")
sys.exit(0 if ok else 1)
EOF

echo
if [ "$fails" = 0 ]; then echo "labels smoke: ok"; else echo "labels smoke: $fails MISMATCH(es)"; exit 1; fi
