#!/usr/bin/env bash
# examples/cortex/run.sh — `cortex locate` and `cortex repair` on pricing.ax.
#
# Every case runs on its own temp copy of pricing.ax, prints the expected and
# the actual exit code, and the script exits non-zero if any case differs.
# docs/CORTEX.md walks through the same commands.
#
# Binaries: the ones named in $AXON and $CORTEX_BIN, or else the ones this
# script builds (scripts/lib/axon_bin.sh — never a binary that merely sits in
# target/ or on PATH).
#
#   bash examples/cortex/run.sh                 # deterministic cases, no model
#   REAL_MODEL=1 bash examples/cortex/run.sh    # plus ONE episode driven by a
#                                               # real model through the `cmd:`
#                                               # seam (needs the `claude` CLI,
#                                               # logged in; costs money)
#   DECISION_MODEL=1 bash examples/cortex/run.sh
#                                               # plus the model ranker and the
#                                               # post-fix audit, once on Haiku 5.5
#                                               # and once on Mercury Decide
#                                               # (needs OPENROUTER_API_KEY and a
#                                               # cortex built with `--features ai`;
#                                               # under $0.01 a run)
#
# The deterministic cases pin CORTEX_RANKER=spectrum and CORTEX_AUDIT=off, so a
# key in the environment never sends pricing.ax anywhere unless asked.
set -u
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO" || exit 2
. "$REPO/scripts/lib/axon_bin.sh"

if [ -n "${AXON:-}" ]; then
  named_bin AXON cortex-example
else
  cargo build -q -p axon-core --no-default-features --bin axon || exit 2
  use_built AXON axon
fi
if [ -n "${CORTEX_BIN:-}" ]; then
  named_bin CORTEX_BIN cortex-example
else
  if [ "${DECISION_MODEL:-0}" = "1" ]; then
    cargo build -q -p axon-cortex --features ai --bin cortex || exit 2
  else
    cargo build -q -p axon-cortex --bin cortex || exit 2
  fi
  use_built CORTEX_BIN cortex
fi
export CORTEX_RANKER=spectrum CORTEX_AUDIT=off

SRC="$REPO/examples/cortex/pricing.ax"
CHECK=hidden_discount_boundary
FIX='if total >= 100 { total / 10 } else { 0 }'
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
fails=0
n=0

# fresh — a new workspace holding an untouched copy of pricing.ax; sets $W.
fresh() {
  n=$((n + 1))
  W="$WORK/case$n"
  mkdir -p "$W"
  cp "$SRC" "$W/pricing.ax"
}

# expect CODE NAME -- CMD... — run CMD, compare its exit code with CODE.
expect() {
  local want="$1" name="$2"
  shift 3
  echo
  echo "== $name"
  printf '$ cortex'
  printf ' %q' "${@:2}"
  printf '\n'
  "$@"
  local got=$?
  if [ "$got" = "$want" ]; then
    echo "-- expected exit $want, got $got: ok"
  else
    echo "-- expected exit $want, got $got: MISMATCH"
    fails=$((fails + 1))
  fi
}

# same_as_source NAME — the workspace copy must be byte-identical to pricing.ax.
same_as_source() {
  if cmp -s "$SRC" "$W/pricing.ax"; then
    echo "-- $1: pricing.ax is byte-identical to the original: ok"
  else
    echo "-- $1: pricing.ax was changed: MISMATCH"
    fails=$((fails + 1))
  fi
}

common=(--file pricing.ax --check "$CHECK" --axon "$AXON")

# 1. Rank the functions the defect may be in. The adjudicator is excluded from
#    the evidence, so only visible_boundary_order counts as failing.
fresh
expect 0 "locate" -- "$CORTEX_BIN" locate --workspace "$W" "${common[@]}"

# 2. A known-good body, through the full pipeline. Exit 0 means the hidden
#    check accepted it AND no check in the file still fails.
fresh
expect 0 "repair with literal: (correct body)" -- \
  "$CORTEX_BIN" repair --workspace "$W" "${common[@]}" \
  --write-prefix pricing.ax --generator "literal:$FIX"
if grep -q 'total >= 100' "$W/pricing.ax" && "$AXON" test "$W/pricing.ax" >/dev/null; then
  echo "-- the repaired file keeps the patch and all its tests pass: ok"
else
  echo "-- the repaired file is not clean: MISMATCH"
  fails=$((fails + 1))
fi

# 3. A body that compiles but is wrong (discounts every order). The loop
#    proposes it, the hidden check rejects it, the same body comes back, and
#    stuck-detection ends the episode: exit 21. The file is restored.
fresh
expect 21 "repair with a wrong literal: (compiles, still wrong)" -- \
  "$CORTEX_BIN" repair --workspace "$W" "${common[@]}" --symbol discount \
  --write-prefix pricing.ax --generator "literal:total / 10"
same_as_source "after the failed repair"

# 4. A check that already passes cannot witness a repair: refused before any
#    episode runs.
fresh
expect 26 "--check names a test that already passes" -- \
  "$CORTEX_BIN" repair --workspace "$W" --file pricing.ax \
  --check visible_small_order --axon "$AXON" \
  --write-prefix pricing.ax --generator "literal:$FIX"
same_as_source "after the refusal"

# 5. The grant does not cover the file: refused before the generator is asked.
fresh
expect 23 "--write-prefix does not cover pricing.ax" -- \
  "$CORTEX_BIN" repair --workspace "$W" "${common[@]}" \
  --write-prefix src/ --generator "literal:$FIX"
same_as_source "after the refusal"

# 6. A body that ends the test process with `exit(0)` before any assertion
#    runs. It must NOT be accepted: plain `axon test` reports a test ended by
#    exit(0) as ok, so cortex's checker runs every check with a per-run
#    completion key and counts a test as passed only with its token. The
#    candidate never completes the hidden check: no progress, file restored.
fresh
expect 21 "a body that calls exit(0) must not verify" -- \
  "$CORTEX_BIN" repair --workspace "$W" "${common[@]}" --symbol discount \
  --write-prefix pricing.ax --generator "literal:exit(0)
    0"
same_as_source "after the rejection"

# 7. Optional: a real model writes the body. Its outcome is not fixed, so it
#    is reported, not asserted.
if [ "${REAL_MODEL:-0}" = "1" ]; then
  fresh
  DRIVER="$REPO/crates/axon-cortex/benchmarks/drivers/claude_cli.sh"
  export CORTEX_MODEL_LOG="$WORK/model-log.jsonl"
  echo
  echo "== REAL_MODEL: repair with cmd:$DRIVER (model ${CORTEX_MODEL:-claude-haiku-4-5-20251001})"
  # From inside the temp workspace: the driver inherits cortex's cwd, and
  # `claude` started in the repository root would load its CLAUDE.md into
  # every proposal's context.
  (cd "$W" && "$CORTEX_BIN" repair --workspace "$W" "${common[@]}" \
    --write-prefix pricing.ax --generator "cmd:$DRIVER")
  echo "-- exit $? (not asserted)"
  echo "-- discount() now reads:"
  grep -A2 '^fn discount' "$W/pricing.ax"
  echo "-- model log (one line per proposal):"
  python3 -c '
import json, sys
total = 0.0
for line in open(sys.argv[1]):
    r = json.loads(line)
    total += r["cost_usd"] or 0
    print("   candidate=%s proposal=%s cost_usd=%s tokens_out=%s extraction=%s body=%r"
          % (r["candidate"], r["proposal_index_for_candidate"], r["cost_usd"],
             r["output_tokens"], r["extraction_status"], r["body"]))
print("   total cost_usd=%.4f" % total)
' "$CORTEX_MODEL_LOG"
fi

# 8. Optional: the decision models around the walk. The generator is the
#    known-good literal, so the exit code is fixed (0) whatever order the
#    ranker picks; what is asserted is that the ranker ORDERED the walk and the
#    audit ANSWERED (no fallback to spectrum order, no `unavailable`). The
#    order, the audit verdict, latency and cost are reported, not asserted.
if [ "${DECISION_MODEL:-0}" = "1" ]; then
  if [ -z "${OPENROUTER_API_KEY:-}" ]; then
    echo
    echo "== DECISION_MODEL: OPENROUTER_API_KEY is not set: MISMATCH"
    fails=$((fails + 1))
  else
    for spec in openrouter:anthropic/claude-haiku-5.5 decision:inception/mercury-decide; do
      fresh
      echo
      echo "== DECISION_MODEL: --ranker $spec --audit $spec"
      "$CORTEX_BIN" repair --workspace "$W" "${common[@]}" \
        --write-prefix pricing.ax --generator "literal:$FIX" \
        --ranker "$spec" --audit "$spec" --json >"$W/outcome.json"
      got=$?
      if [ "$got" = 0 ]; then
        echo "-- expected exit 0, got 0: ok"
      else
        echo "-- expected exit 0, got $got: MISMATCH"
        fails=$((fails + 1))
      fi
      if python3 - "$W/outcome.json" "$spec" <<'EOF'
import json, sys
lines = [l for l in open(sys.argv[1]) if l.strip()]
o = json.loads(lines[-1]) if lines else {}
spec = sys.argv[2]
r, a, e = o.get("ranker") or {}, o.get("audit") or {}, o.get("escalation") or {}
print("-- ranker: used=%s order=%s latency_ms=%s cost_usd=%s reason=%s"
      % (r.get("used"), r.get("order"), r.get("latency_ms"), r.get("cost_usd"), r.get("reason")))
print("-- attempted: %s" % o.get("attempted"))
print("-- audit: verdict=%s p=%s latency_ms=%s cost_usd=%s reason=%s"
      % (a.get("verdict"), a.get("p"), a.get("latency_ms"), a.get("cost_usd"), a.get("reason")))
print("-- escalation: %s" % e.get("advice"))
ok = r.get("used") == spec and a.get("verdict") in ("clean", "suspect")
print("-- the ranker ordered the walk and the audit answered: %s" % ("ok" if ok else "MISMATCH"))
sys.exit(0 if ok else 1)
EOF
      then :; else fails=$((fails + 1)); fi
    done
  fi
fi

echo
if [ "$fails" -eq 0 ]; then
  echo "cortex example: all cases matched"
else
  echo "cortex example: $fails mismatch(es)"
  exit 1
fi
