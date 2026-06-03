#!/usr/bin/env bash
# goal_input_parity.sh — F11 codegen regression.
#
# F11: native codegen used to log only the @[adaptive] return SCORE, not the
# INPUT that produced it, so native goal_run always cold-started its hill-climb
# at x=0 (the interpreter already logs (input, score) and warm-starts). The fix
# threads the adaptive fn's leading i64 param into a new runtime entry point
# (__axon_provenance_log_ret_i64_in). This harness builds a native @[adaptive]
# fn(i64)->i64 and asserts its provenance JSONL now carries the input, matching
# the interpreter — so the ABI thread can't silently regress.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/adaptive.ax"
cat > "$PROG" <<'AX'
@[adaptive]
fn tune(x: i64) -> i64 { 100 - (x - 7) * (x - 7) }

fn main() -> i64 {
    let _ = tune(3)
    let _ = tune(7)
    0
}
AX

echo "goal_input_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "goal_input_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# Interpreter provenance (the oracle — already logs input).
IPROV="$WORK/icache"; mkdir -p "$IPROV"
XDG_CACHE_HOME="$IPROV" AXON_AI_MOCK=1 AXON_SEED=42 "$AXON" run "$PROG" >/dev/null 2>&1
ILOG="$IPROV/axon/provenance.jsonl"

# Native provenance.
NPROV="$WORK/ncache"; mkdir -p "$NPROV"
BIN="$WORK/adaptive_bin"
if ! XDG_CACHE_HOME="$NPROV" "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "goal_input_parity: native build failed — skipping"
  exit 0
fi
XDG_CACHE_HOME="$NPROV" "$BIN" >/dev/null 2>&1
NLOG="$NPROV/axon/provenance.jsonl"

if [ ! -f "$NLOG" ]; then
  echo "goal_input_parity: FAIL — native wrote no provenance.jsonl"
  exit 1
fi

# The native return records must now carry the input as a STRUCTURED field
# ("input":<n>, matching the interpreter — R4 parity), not buried in payload.
# tune(3) -> 84, tune(7) -> 100; assert both input bindings are present.
if ! grep -q '"input":3' "$NLOG" || ! grep -q '"input":7' "$NLOG"; then
  echo "goal_input_parity: FAIL — native provenance is missing the structured input field (F11/R4 ABI regressed):"
  echo "--- native log ---"; cat "$NLOG"
  exit 1
fi

# And the (input -> score) pairing must match the interpreter's: tune(3)=84.
if ! grep -q '"score":84,"input":3' "$NLOG"; then
  echo "goal_input_parity: FAIL — native (input,score) pairing wrong: expected score 84 with input 3"
  echo "--- native log ---"; cat "$NLOG"
  exit 1
fi

echo "goal_input_parity: OK — native provenance carries (input, score), matching the interpreter:"
grep "adaptive_return" "$NLOG" | sed 's/^/  /'
echo "native adaptive provenance carries the input"
exit 0
