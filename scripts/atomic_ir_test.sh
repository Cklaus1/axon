#!/usr/bin/env bash
# R17 Slice 2 acceptance gate (golden-IR proxy): the SMP atomic builtins lower
# to the corresponding LLVM atomic instruction WITH THE NAMED MEMORY ORDER.
#
# This is the codegen unit gate for `axon_smp_atomic_counter_is_race_free`.
# A true 2-core QEMU SMP harness (boot 2 APs, both hammer a shared counter,
# assert the exact final value) is heavier infra; the golden-IR check proves
# the load-bearing soundness property: the increment is a single `atomicrmw add`
# with `seq_cst` ordering, so no two cores can lose an update. The actual
# race-freedom is an LLVM/hardware guarantee given that instruction.
#
# Requires: axon (with codegen). Skips gracefully if codegen is absent.
# Usage: scripts/atomic_ir_test.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

skip() { echo "SKIP: $*" >&2; exit 0; }
fail() { echo "FAIL: $*" >&2; exit 1; }

AXON_BIN=""
for candidate in \
    "$REPO/target/debug/axon" \
    "$REPO/target/release/axon" \
    "$(command -v axon 2>/dev/null || true)"
do
    if [[ -x "$candidate" ]]; then AXON_BIN="$candidate"; break; fi
done
[[ -n "$AXON_BIN" ]] || skip "axon binary not found (build with: cargo build -p axon-core)"

# No --help-based codegen probe: --emit-llvm is listed unconditionally, so
# flag-presence never distinguishes a codegen build from a --no-default-features
# one. The real probe is the trial build below.

SRC="$REPO/examples/kernel/hello_kernel_slice2.ax"
[[ -f "$SRC" ]] || fail "missing example $SRC"

# Robust codegen detection: a binary built --no-default-features still parses
# the --emit-llvm flag (it lives in the clap enum) but prints a "requires the
# codegen feature" error and exits 1. Probe with a trivial build and SKIP if so,
# so the test passes under the interpreter-only `cargo test --no-default-features`.
PROBE="$(mktemp /tmp/axon_probe.XXXXXX.ax)"
PROBE_LL="${PROBE%.ax}.ll"
printf 'fn main() { let _x = 1 }\n' > "$PROBE"
probe_out="$("$AXON_BIN" build --emit-llvm "$PROBE" --out "$PROBE_LL" 2>&1 || true)"
rm -f "$PROBE" "$PROBE_LL"
if echo "$probe_out" | grep -q "requires building axon with the \`codegen\` feature"; then
    skip "axon binary built without the codegen feature (use: cargo build -p axon-core)"
fi

IR="$(mktemp /tmp/axon_atomic_ir.XXXXXX.ll)"
trap 'rm -f "$IR"' EXIT

"$AXON_BIN" build --freestanding --emit-llvm "$SRC" --out "$IR" >/dev/null 2>&1 \
    || fail "axon build --emit-llvm failed"

# Each pattern is a load-bearing (instruction, memory-order) pair. A regression
# in lowering (wrong order, non-atomic op) fails the exact-string match.
declare -a PATTERNS=(
    'atomicrmw add ptr .* i64 1 seq_cst'          # counter_inc → race-free SMP add
    'load atomic i64, ptr .* acquire'             # counter_read
    'store atomic i64 0, ptr .* release'          # counter_reset
    'cmpxchg ptr .* i64 0, i64 1 seq_cst'         # lock_acquire CAS
)

matched=0
for pat in "${PATTERNS[@]}"; do
    if grep -Eq "$pat" "$IR"; then
        matched=$((matched + 1))
    else
        echo "--- emitted IR (atomic lines) ---" >&2
        grep -nE 'atomicrmw|load atomic|store atomic|cmpxchg' "$IR" >&2 || true
        fail "expected IR pattern not found: /$pat/"
    fi
done

# Sweep guard (vacuous-pass): we must have actually matched every pattern.
if [[ "$matched" -ne "${#PATTERNS[@]}" ]]; then
    fail "only matched $matched/${#PATTERNS[@]} atomic IR patterns"
fi

echo "PASS: all ${#PATTERNS[@]} atomic IR lowerings present with correct memory order"
exit 0
