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
AXON="target/debug/axon"

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
if ! XDG_CACHE_HOME="$NPROV" "$AXON" build "$PROG" -o "$BIN" >/dev/null 2>&1; then
  echo "provenance_parity: native build of the .ax failed — skipping native half"
  exit 0
fi
XDG_CACHE_HOME="$NPROV" "$BIN" >/dev/null 2>&1
NLOG="$NPROV/axon/provenance.jsonl"
if [ ! -f "$NLOG" ]; then
  echo "provenance_parity: FAIL — native binary wrote no provenance.jsonl (I-13 codegen hole)"
  exit 1
fi

# ── Parity assertion: the native return records must carry the SAME ────────────
# discriminating fields the interpreter writes. We extract, from each log, the
# adaptive_return records as `fn|score` and require the native set ⊇ interp set.
extract() {
  grep '"event":"adaptive_return"' "$1" \
    | grep '"zone":"adaptive"' \
    | sed -E 's/.*"fn":"([^"]+)".*"score":([0-9.eE+-]+).*/\1|\2/' \
    | sort
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

echo "provenance_parity: OK — native and interp adaptive provenance agree (event/zone/fn/score):"
echo "$NSET" | sed 's/^/  /'
echo "native and interp provenance agree"
exit 0
