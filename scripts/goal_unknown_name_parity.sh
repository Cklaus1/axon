#!/usr/bin/env bash
# goal_unknown_name_parity.sh — BUG_HUNT #19 codegen regression (I-9 parity).
#
# #19: `goal_run("typo", …)` against a name that is neither a defined fn nor a
# recorded-provenance key is a misspelled metric. The interpreter aborts
# (`axon: panic: goal function … (typo?)`, exit 101) so a typo can't masquerade
# as an achieved goal — but native codegen used to silently return `target`,
# making the bug invisible in AOT builds. This harness builds a program that
# calls `goal_run` with a typo'd name BOTH ways and asserts they now AGREE:
# same panic message, same exit 101. It also checks the happy path (correct
# adaptive name → identical score) so the guard can't over-reject.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Program (a): typo'd goal name — must abort in BOTH engines.
TYPO="$WORK/typo.ax"
cat > "$TYPO" <<'AX'
@[adaptive]
fn tune(x: i64) -> i64 { 100 - (x - 7) * (x - 7) }

fn main() -> i64 {
    let best = goal_run("tunee", 100.0, 32)
    println("best {to_str(f64_to_i64(best))}")
    0
}
AX

# Program (b): correct goal name — must SUCCEED identically in both engines.
OK="$WORK/ok.ax"
cat > "$OK" <<'AX'
@[adaptive]
fn tune(x: i64) -> i64 { 100 - (x - 7) * (x - 7) }

fn main() -> i64 {
    let best = goal_run("tune", 100.0, 32)
    println("best {to_str(f64_to_i64(best))}")
    0
}
AX

echo "goal_unknown_name_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "goal_unknown_name_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# Run the interpreter, preserving its exit code, then strip the Phase-9
# `axon: run-id` stderr stamp (the native binary emits no such line, so leaving
# it in would be a false divergence). A bare pipe would clobber $? with grep's.
run_interp() {
  local out; out="$(AXON_AI_MOCK=1 AXON_SEED=42 "$AXON" run "$1" 2>&1)"; local code=$?
  printf '%s\n' "$out" | grep -v '^axon: run-id '
  return $code
}

build_native() {
  local src="$1" bin="$2"
  AXON_AI_MOCK=1 "$AXON" build "$src" -o "$bin" --no-cache >/dev/null 2>&1
}

# ── (a) typo: both engines must panic + exit 101 with the same message ────────
I_OUT="$(run_interp "$TYPO")"; I_EXIT=$?
TBIN="$WORK/typo_bin"
if ! build_native "$TYPO" "$TBIN"; then
  echo "goal_unknown_name_parity: native build failed — skipping"
  exit 0
fi
N_OUT="$(AXON_AI_MOCK=1 "$TBIN" 2>&1)"; N_EXIT=$?

if [ "$I_EXIT" -ne 101 ]; then
  echo "FAIL: interp exit on typo was $I_EXIT, expected 101"; exit 1
fi
if [ "$N_EXIT" -ne 101 ]; then
  echo "FAIL: native exit on typo was $N_EXIT, expected 101 (silently returned target?)"; exit 1
fi
if [ "$I_OUT" != "$N_OUT" ]; then
  echo "FAIL: typo message diverges"; echo "  interp: $I_OUT"; echo "  native: $N_OUT"; exit 1
fi
case "$I_OUT" in
  *"is not defined and has no recorded provenance"*) : ;;
  *) echo "FAIL: typo message missing the typo hint: $I_OUT"; exit 1 ;;
esac
echo "goal_unknown_name_parity: typo path OK — both abort exit 101, identical message"

# ── (b) correct name: both engines must succeed with the same score ───────────
IO_OUT="$(run_interp "$OK")"; IO_EXIT=$?
OBIN="$WORK/ok_bin"
build_native "$OK" "$OBIN" || { echo "goal_unknown_name_parity: ok native build failed — skipping"; exit 0; }
NO_OUT="$(AXON_AI_MOCK=1 "$OBIN" 2>&1)"; NO_EXIT=$?

if [ "$IO_EXIT" -ne 0 ] || [ "$NO_EXIT" -ne 0 ]; then
  echo "FAIL: correct name should exit 0 (interp=$IO_EXIT native=$NO_EXIT)"; exit 1
fi
if [ "$IO_OUT" != "$NO_OUT" ]; then
  echo "FAIL: correct-name output diverges"; echo "  interp: $IO_OUT"; echo "  native: $NO_OUT"; exit 1
fi
echo "goal_unknown_name_parity: happy path OK — both exit 0, identical output ($IO_OUT)"

echo "goal_unknown_name_parity: PASS"
exit 0
