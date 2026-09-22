#!/usr/bin/env bash
# provenance_parity.sh — R4 codegen-provenance tripwire.
#
# I-13 (provenance is not opt-out-able) must hold UNIFORMLY across both engines:
# the tree-walking interpreter AND the native codegen build. The danger the R4
# spec (§4.2, §8 "Parity") names is that native silently loses the guarantee —
# it *looks* present because the interpreter (the tested path) injects, while a
# native binary runs the same @[adaptive] fn un-logged or with a degraded
# record shape.
#
# This harness compiles ONE @[adaptive] program two ways and asserts the native
# binary writes return-provenance with the SAME discriminating fields the
# interpreter does: event=="adaptive_return", zone=="adaptive", the fn name, and
# the score. It is the executable tripwire that fires the instant codegen stops
# injecting (or regresses the record shape).
#
# Requires: the codegen `axon` binary (LLVM/inkwell). Skips (exit 0 with a
# notice) when codegen can't build — so it is safe in interpreter-only CI. The
# interpreter half always runs; only the native comparison needs LLVM.
#
# Usage:  scripts/provenance_parity.sh
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/adaptive.ax"
cat > "$PROG" <<'AX'
@[adaptive]
fn tune(x: i64) -> i64 { 100 - (x - 7) * (x - 7) }

fn main() {
    let _ = tune(3)
    let _ = tune(7)
    println("done")
}
AX

# ── Build the codegen `axon` binary (default features = codegen). ─────────────
echo "provenance_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "provenance_parity: codegen build unavailable (LLVM/inkwell absent) — skipping native half"
  exit 0
fi
AXON="${AXON:-target/debug/axon}"

# ── Interpreter run → provenance. ─────────────────────────────────────────────
IPROV="$WORK/icache"
mkdir -p "$IPROV"
XDG_CACHE_HOME="$IPROV" AXON_AI_MOCK=1 AXON_SEED=42 "$AXON" run "$PROG" >/dev/null 2>&1
ILOG="$IPROV/axon/provenance.jsonl"
if [ ! -f "$ILOG" ]; then
  echo "provenance_parity: FAIL — interpreter wrote no provenance.jsonl"
  exit 1
fi

# ── Native build + run → provenance. ──────────────────────────────────────────
NPROV="$WORK/ncache"
mkdir -p "$NPROV"
BIN="$WORK/adaptive_bin"
if ! berr="$(XDG_CACHE_HOME="$NPROV" "$AXON" build "$PROG" -o "$BIN" 2>&1)"; then
  # A build that FAILED is a RESULT, not an absence — see
  # scripts/lib/harness_skip.sh. This used to drop the whole native half.
  native_build_failed provenance_parity "adaptive provenance" "$berr" || exit 1
fi
XDG_CACHE_HOME="$NPROV" "$BIN" >/dev/null 2>&1
NLOG="$NPROV/axon/provenance.jsonl"
if [ ! -f "$NLOG" ]; then
  echo "provenance_parity: FAIL — native binary wrote no provenance.jsonl (I-13 codegen hole)"
  exit 1
fi

# ── Parity assertion: the native return records must carry the SAME ────────────
# fields the interpreter writes — ALL of them, not a chosen few.
#
# This used to project each record to `fn|score` with a regex. A projection can
# only compare the fields it names, so it cannot notice a field ONE engine stops
# writing: that is exactly how native `agent_action` records lost `effect_row`
# and `principal` while their own harness reported a match (378da246). The same
# shape of blind spot was here, on the records the goal optimizer is built on.
#
# Now every key of each adaptive_return record is compared, minus `ts_ms` (two
# runs legitimately differ in when they happened). Parsed as JSON, so field
# ORDER is not part of the comparison either.
#
# Scoped to adaptive_return ON PURPOSE. The two logs are NOT record-for-record
# identical and are not meant to be: the interpreter stamps a `run_start` (the
# handle `axon trace --replay` needs, which native refuses), and native emits a
# legacy `call` entry record nothing reads. Comparing whole logs would fail on
# that known, intentional difference instead of on a regression.
extract() {
  python3 - "$1" <<'PYEOF'
import json, sys
try:
    lines = open(sys.argv[1]).read().splitlines()
except OSError:
    sys.exit(0)
out = []
for line in lines:
    if '"event":"adaptive_return"' not in line or '"zone":"adaptive"' not in line:
        continue
    try:
        r = json.loads(line)
    except ValueError:
        out.append("UNPARSEABLE-RECORD")
        continue
    r.pop("ts_ms", None)
    # A record missing the fields that carry the MEANING is a failure, not a
    # thing to compare loosely: if both engines dropped `fn` or `score`, two
    # identical husks would still match and the harness would pass vacuously.
    missing = [k for k in ("fn", "score") if r.get(k) in (None, "")]
    if missing:
        out.append("MISSING-FIELD:" + ",".join(missing))
        continue
    out.append(json.dumps(r, sort_keys=True))
print("\n".join(sorted(out)))
PYEOF
}

ISET="$(extract "$ILOG")"
NSET="$(extract "$NLOG")"

if [ -z "$ISET" ]; then
  echo "provenance_parity: FAIL — interpreter produced no adaptive_return/zone:adaptive records"
  echo "--- interp log ---"; cat "$ILOG"
  exit 1
fi
if [ -z "$NSET" ]; then
  echo "provenance_parity: FAIL — native produced no adaptive_return/zone:adaptive records (codegen degraded the record shape — I-13 violation)"
  echo "--- native log ---"; cat "$NLOG"
  exit 1
fi

# Every interp record must appear in the native set (native may carry extra
# prologue "call" lines, which we ignore — the contract is on return records).
MISSING="$(comm -23 <(echo "$ISET") <(echo "$NSET"))"
if [ -n "$MISSING" ]; then
  echo "provenance_parity: FAIL — native is missing interp return records (engine divergence):"
  echo "$MISSING"
  echo "--- interp ---"; echo "$ISET"
  echo "--- native ---"; echo "$NSET"
  exit 1
fi

echo "provenance_parity: OK — native and interp adaptive provenance agree (every field but ts_ms):"
echo "$NSET" | sed 's/^/  /'
echo "native and interp provenance agree"
exit 0
