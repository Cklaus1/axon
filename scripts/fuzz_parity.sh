#!/usr/bin/env bash
# fuzz_parity.sh — R1f slice 1: differential parity fuzzing (auto-find
# interp↔codegen divergence). See governance/specs/R1f-differential-parity-fuzz.md.
#
# Unlike the 21 hand-written fixed-case harnesses, this one SEARCHES the input
# space: per builtin it generates N seeded-random inputs PLUS the edge values
# where divergence hides (0, ±1, large magnitudes), emits ONE .ax that prints
# f(input) for every input, builds it ONCE (fork (b) — amortize the LLVM link),
# runs both engines, and diffs stdout line-by-line + exit code.
#
# Determinism: the input generator is seeded from AXON_SEED (default 42), so a
# divergence is reproducible and the gate never flakes. Edge values are always
# included regardless of seed.
#
# SLICE 1 SCOPE: the skeleton + 3 descriptors (abs_i64, min_i64, the `+` binop).
# Inputs are bounded to the NON-overflowing domain on purpose — the i64-overflow
# boundary (where interp's checked/saturating arithmetic and codegen's wrapping
# IR are KNOWN to diverge, e.g. the documented arr_sum case) is a slice-2 target
# with ExitCode/explicit comparison, not a slice-1 surprise. Slice 2 widens the
# descriptor table to ~30-40 scalar/str/math builtins; slice 3 is automatic —
# this file matches scripts/*_parity.sh so parity_all.sh already runs it.
#
# Skips (exit 0) when the codegen toolchain is absent (same contract as the
# other harnesses). Exit nonzero on a real divergence.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SEED="${AXON_SEED:-42}"
N="${FUZZ_N:-40}"   # random inputs per builtin (edges added on top)

# Locate the codegen binary. Prefer an already-built one (the gate builds it
# before tests); if absent try one build; if THAT fails (no LLVM / build lock
# held by a parent cargo), skip cleanly rather than report a false divergence.
AXON="target/debug/axon"
if [ ! -x "$AXON" ]; then
  if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
    echo "fuzz_parity: codegen build unavailable (LLVM absent or build lock) — skipping"; exit 0
  fi
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Probe: can this binary actually emit native code? (A --no-default-features
# build leaves a codegen-less `axon` that can `run` but not `build`.)
printf 'fn main() -> i64 { 0 }\n' > "$WORK/probe.ax"
if ! "$AXON" build "$WORK/probe.ax" -o "$WORK/probe.bin" --no-cache >/dev/null 2>&1; then
  echo "fuzz_parity: this axon binary cannot emit native builds (no codegen feature) — skipping"; exit 0
fi

fail=0

# fuzz NAME ARITY EXPR
#   EXPR is an Axon expression template using `A` (and `B` for arity 2) as the
#   integer placeholders, e.g. 'abs_i64(A)' or 'min_i64(A, B)' or 'A + B'.
#   Generates edges+N seeded inputs, emits one println(to_str(EXPR)) per input,
#   builds once, asserts interp stdout+exit == native stdout+exit.
fuzz() {
  local name="$1" arity="$2" expr="$3"
  local src="$WORK/$name.ax"

  # Generate the program body: one line per input. awk owns RNG (seeded) so the
  # set is reproducible; edge values are prepended unconditionally. Inputs are
  # bounded to ±1e9 → every abs/min/+ stays within i64 (no overflow in slice 1).
  {
    echo "fn main() -> i64 {"
    awk -v seed="$SEED" -v n="$N" -v arity="$arity" -v expr="$expr" 'BEGIN {
      srand(seed)
      ne = split("0 1 -1 2 -2 1000000000 -1000000000 999999999 -999999999", edges, " ")
      cnt = 0
      for (i = 1; i <= ne; i++) { cnt++; vals[cnt] = edges[i] }
      for (i = 1; i <= n; i++) { cnt++; vals[cnt] = int(rand() * 2000000000) - 1000000000 }
      for (i = 1; i <= cnt; i++) {
        e = expr
        gsub(/A/, vals[i], e)
        if (arity == 2) {
          # deterministic second operand from a different index in the table
          b = vals[((i * 7) % cnt) + 1]
          gsub(/B/, b, e)
        }
        print "    println(to_str(" e "))"
      }
    }'
    echo "    0"
    echo "}"
  } > "$src"

  # interp (the oracle)
  local i_out i_exit n_out n_exit
  i_out="$("$AXON" run "$src" 2>/dev/null)"; i_exit=$?

  # native (build once, run)
  if ! "$AXON" build "$src" -o "$WORK/$name.bin" --no-cache >/dev/null 2>&1; then
    echo "  FAIL $name: native build failed"; fail=1; return
  fi
  n_out="$("$WORK/$name.bin" 2>/dev/null)"; n_exit=$?

  if [ "$i_exit" != "$n_exit" ]; then
    echo "  FAIL $name: exit interp=$i_exit native=$n_exit"; fail=1; return
  fi
  if [ "$i_out" != "$n_out" ]; then
    echo "  FAIL $name: stdout divergence — first differing line:"
    diff <(printf '%s\n' "$i_out") <(printf '%s\n' "$n_out") | head -6 | sed 's/^/      /'
    fail=1; return
  fi
  local cases=$(printf '%s\n' "$i_out" | wc -l)
  echo "  OK   $name: $cases inputs, interp==native (exit $i_exit)"
}

echo "fuzz_parity: seed=$SEED, $((N)) random + 9 edge inputs per builtin"
# ── slice-1 descriptors: {name, arity, expr} ──────────────────────────────────
fuzz abs_i64 1 'abs_i64(A)'      # unary extern builtin (registry row)
fuzz min_i64 2 'min_i64(A, B)'   # binary extern builtin (registry row)
fuzz add_i64 2 'A + B'           # inline i64 binop lowering (emit_binop)

[ "$fail" -eq 0 ] || { echo "fuzz_parity: FAIL — interp↔codegen divergence found"; exit 1; }
echo "fuzz_parity: PASS — abs_i64 / min_i64 / + agree native==interp on edges + seeded random ✓"
exit 0
