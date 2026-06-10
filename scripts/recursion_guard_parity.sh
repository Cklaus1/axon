#!/usr/bin/env bash
# recursion_guard_parity.sh — native↔interp EXIT-CODE parity on the recursion
# (stack-overflow) fault.
#
# The interpreter bounds recursion with a depth counter and panics gracefully
# (exit 101, "recursion limit exceeded …") on runaway recursion. Native code runs
# on the OS stack, so the same program used to SIGSEGV (exit 139, no diagnostic) —
# a poor failure mode for a safety-first language (and for AI-authored code, which
# may recurse without bound). A SIGSEGV handler on an alternate stack
# (`__axon_install_recursion_guard`, called from codegen-emitted `main`) now
# converts the overflow into the SAME exit code (101) plus a "stack overflow"
# diagnostic. This harness pins that exit-code parity so it can't regress.
#
# NOTE: unlike checked_arith_parity, the *messages* are NOT byte-identical — native
# can't know the depth/fn at the overflow point — so we assert the EXIT CODE
# matches (101) and that each engine prints an appropriate diagnostic. A normal
# program and legitimate finite recursion must be UNAFFECTED by the guard.
#
# Requires the codegen `axon` binary (LLVM) + a unix host. Skips (exit 0) otherwise.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

case "$(uname -s)" in
  Linux|Darwin|*BSD) ;;
  *) echo "recursion_guard_parity: non-unix host (guard is a no-op) — skipping"; exit 0 ;;
esac

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "recursion_guard_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "recursion_guard_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
cargo build -q -p axon-rt 2>/dev/null || true
AXON="target/debug/axon"

# (1) Infinite recursion — must panic gracefully (exit 101) on BOTH engines.
REC="$WORK/rec.ax"
printf 'fn rec(n: i64) -> i64 { rec(n + 1) }\nfn main() -> i64 { rec(0) }\n' > "$REC"

iout="$("$AXON" run "$REC" 2>&1)"; iexit=$?
if [ "$iexit" -ne 101 ] || ! printf '%s' "$iout" | grep -q "recursion limit"; then
  echo "recursion_guard_parity: FAIL — interp should panic 101 'recursion limit', got exit=$iexit: $iout"; exit 1
fi

BIN="$WORK/rec_bin"
if ! "$AXON" build "$REC" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "recursion_guard_parity: native build/link unavailable in this env — skipping"; exit 0
fi
nout="$("$BIN" 2>&1)"; nexit=$?
if [ "$iexit" -ne "$nexit" ]; then
  echo "recursion_guard_parity: FAIL — exit-code divergence: interp=$iexit native=$nexit"
  echo "  native out: $nout"; exit 1
fi
if ! printf '%s' "$nout" | grep -q "stack overflow"; then
  echo "recursion_guard_parity: FAIL — native overflow must print a 'stack overflow' diagnostic, got: $nout"; exit 1
fi
echo "  infinite recursion: interp==native (exit $iexit, graceful) ✓"

# (2) A normal program must be UNAFFECTED (the guard only catches SIGSEGV).
OK="$WORK/ok.ax"
printf 'fn main() -> i64 { println("hi")  0 }\n' > "$OK"
"$AXON" build "$OK" -o "$WORK/ok_bin" --no-cache >/dev/null 2>&1
okout="$("$WORK/ok_bin" 2>&1)"; okexit=$?
if [ "$okexit" -ne 0 ] || [ "$okout" != "hi" ]; then
  echo "recursion_guard_parity: FAIL — normal program disturbed by the guard: exit=$okexit out=[$okout]"; exit 1
fi
echo "  normal program: unaffected (exit 0) ✓"

# (3) Legitimate finite recursion must compute correctly (guard fires only on a
# real overflow, not at an artificial limit). sum(1..5000) = 12502500 → exit
# 12502500 % 256 = 228.
SUM="$WORK/sum.ax"
printf 'fn sum(n: i64) -> i64 { if n == 0 { 0 } else { n + sum(n - 1) } }\nfn main() -> i64 { sum(5000) }\n' > "$SUM"
"$AXON" build "$SUM" -o "$WORK/sum_bin" --no-cache >/dev/null 2>&1
"$WORK/sum_bin" >/dev/null 2>&1; sumexit=$?
if [ "$sumexit" -ne 228 ]; then
  echo "recursion_guard_parity: FAIL — finite deep recursion miscomputed: exit=$sumexit (expected 228)"; exit 1
fi
echo "  finite deep recursion (sum 5000): correct (exit 228) ✓"

echo "recursion_guard_parity: OK — native deep recursion fails gracefully (exit 101), interp parity, normals unaffected"
exit 0
