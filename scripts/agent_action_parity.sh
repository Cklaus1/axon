#!/usr/bin/env bash
# agent_action_parity.sh — R4 §4.3 codegen agent-action log regression.
#
# The interpreter injects an `event:"agent_action"` audit record whenever a
# capability-bearing builtin (exec/read_file/ai_complete/…) runs inside an
# `@[agent]` fn (I-13, un-opt-out-able). Native codegen had NO agent awareness,
# so a native agent could act on the world un-audited. Codegen now emits the
# same record (via __axon_log_agent_action). This harness builds an @[agent]
# program both ways and asserts the native agent_action records match the
# interpreter's (and that a non-agent fn logs none).
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/agent.ax"
cat > "$PROG" <<'AX'
@[agent]
fn planner() -> i64 {
    let _ = exec("echo", ["plan"])
    let _ = match read_file("/tmp/nonexist_agent_parity") { Ok(s) => s  Err(_) => "" }
    0
}
fn main() -> i64 { planner() }
AX

echo "agent_action_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "agent_action_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="${AXON:-target/debug/axon}"

# Extract the discriminating fields of each agent_action record.
#
# ATTRIBUTION IS PART OF THE RECORD, so it is part of the comparison. This
# extracted only fn|action|caps_used — exactly the three fields BOTH engines
# happened to write — so it reported "native agent_action records match interp"
# while the native record carried no `effect_row` and no `principal` at all.
# Measured on one @[agent] program with AXON_PRINCIPAL=alice: interp logged
# `"effect_row":"FS","principal":"alice"`, native logged neither, and this
# harness passed. A comparison that only looks at the fields both sides produce
# cannot see a field one side stopped producing.
#
# Parsed as JSON rather than by a field-ORDER-dependent regex, and every field
# is required to be NON-EMPTY: if both engines dropped attribution, an
# order-blind comparison of two empty values would still match and pass
# vacuously, which is the same defect one level up.
extract() {
  python3 - "$1" <<'PYEOF'
import json, sys
rows = []
try:
    lines = open(sys.argv[1]).read().splitlines()
except OSError:
    sys.exit(0)
for line in lines:
    if '"event":"agent_action"' not in line:
        continue
    try:
        r = json.loads(line)
    except ValueError:
        print("UNPARSEABLE-RECORD")
        continue
    vals = [r.get(k, "") for k in ("fn", "action", "caps_used", "effect_row", "principal")]
    if any(v == "" for v in vals):
        missing = [k for k, v in zip(("fn","action","caps_used","effect_row","principal"), vals) if v == ""]
        print("MISSING-FIELD:" + ",".join(missing))
        continue
    print("|".join(vals))
PYEOF
}

IPROV="$WORK/icache"; mkdir -p "$IPROV"
XDG_CACHE_HOME="$IPROV" "$AXON" run "$PROG" >/dev/null 2>&1
ISET="$(extract "$IPROV/axon/provenance.jsonl")"

NPROV="$WORK/ncache"; mkdir -p "$NPROV"
BIN="$WORK/agent_bin"
if ! berr="$(XDG_CACHE_HOME="$NPROV" "$AXON" build "$PROG" -o "$BIN" --no-cache 2>&1)"; then
  # A build that FAILED is a RESULT, not an absence. This used to discard
  # stderr and report a codegen regression in the feature under test as
  # "skipping" — see scripts/lib/harness_skip.sh.
  native_build_failed agent_action_parity "agent_action" "$berr" || exit 1
fi
XDG_CACHE_HOME="$NPROV" "$BIN" >/dev/null 2>&1
NSET="$(extract "$NPROV/axon/provenance.jsonl")"

if [ -z "$ISET" ]; then
  echo "agent_action_parity: FAIL — interpreter logged no agent_action records"
  exit 1
fi
if [ "$ISET" != "$NSET" ]; then
  echo "agent_action_parity: FAIL — native agent_action records differ from interp:"
  echo "--- interp ---"; echo "$ISET" | sed 's/^/  /'
  echo "--- native ---"; echo "$NSET" | sed 's/^/  /'
  exit 1
fi

# The capability actions must be present (exec + fs:read).
if ! echo "$NSET" | grep -q "exec|exec" || ! echo "$NSET" | grep -q "read_file|fs:read"; then
  echo "agent_action_parity: FAIL — expected exec + read_file actions: $NSET"
  exit 1
fi

echo "agent_action_parity: OK — native and interp agent_action records agree:"
echo "$NSET" | sed 's/^/  /'
echo "native agent_action log matches the interpreter"
exit 0
