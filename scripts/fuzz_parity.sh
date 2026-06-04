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

# fuzz NAME DOMAIN ARITY EXPR
#   DOMAIN selects the placeholder generator (A, and B for arity 2):
#     i64  — integers in ±1e9 (edges 0,±1,±2,±1e9,±999999999). NON-overflowing
#            on purpose: the overflow boundary is a known divergence (interp
#            checked/graceful vs native wrapping) — its own ExitCode descriptor.
#     pos  — non-negative i64 in [0,1e6] (for pow/shift/char-index — exponents
#            and bit-counts must stay in range; A^B and 1<<B can't overflow).
#     f64  — decimals; edges 0.0,±1.0,±0.5,large; printed via to_str (which both
#            engines route through the same `%.6g` contract).
#     str  — A is drawn from a fixed corpus of string literals (B too for arity
#            2); the EXPR uses `A`/`B` as already-quoted Axon str expressions.
#   EXPR is an Axon expression template; the result is printed via to_str so a
#   single stdout+exit diff covers every input. Build ONCE, diff both engines.
fuzz() {
  local name="$1" domain="$2" arity="$3" expr="$4"
  local src="$WORK/$name.ax"

  {
    echo "fn main() -> i64 {"
    awk -v seed="$SEED" -v n="$N" -v arity="$arity" -v expr="$expr" -v domain="$domain" 'BEGIN {
      srand(seed)
      # Per-domain value table (edges first, then N seeded-random).
      if (domain == "i64") {
        ne = split("0 1 -1 2 -2 1000000000 -1000000000 999999999 -999999999", edges, " ")
        for (i=1;i<=ne;i++){ cnt++; vals[cnt]=edges[i] }
        for (i=1;i<=n;i++){ cnt++; vals[cnt]=int(rand()*2000000000)-1000000000 }
      } else if (domain == "pos") {
        ne = split("0 1 2 3 10 1000 1000000", edges, " ")
        for (i=1;i<=ne;i++){ cnt++; vals[cnt]=edges[i] }
        for (i=1;i<=n;i++){ cnt++; vals[cnt]=int(rand()*1000000) }
      } else if (domain == "f64") {
        # Edges now SPAN the scientific-notation range (1e6, 1e-7, 1e15, 1e-12)
        # as well as the common range. Slice 2b converged the interp fmt_g onto
        # C %.6g, so the fuzzer proves it across the whole %g domain (mantissa
        # trailing-zero trim + signed two-digit exponent). -0.0 covered by ceil.
        ne = split("0.0 1.0 -1.0 0.5 -0.5 2.5 -2.5 100000.0 -100000.0 1000000.0 -1234567.0 0.0000001 -0.0000001 9999999.0 1.5e15 -2.5e-12", edges, " ")
        for (i=1;i<=ne;i++){ cnt++; vals[cnt]=edges[i] }
        # Random spread across many magnitudes: scale a [-1,1] mantissa by a
        # random power of ten in [-9, 9] so sci-notation is regularly exercised.
        for (i=1;i<=n;i++){
          mant = rand()*2.0 - 1.0
          ex = int(rand()*19) - 9
          cnt++; vals[cnt]=sprintf("%.6e", mant * (10 ^ ex))
        }
      } else if (domain == "str") {
        # A fixed corpus of already-quoted Axon string literals (empty, single
        # char, multi-word with spaces, mixed case, digits, punctuation,
        # substrings) — the cases str-scalar builtins must agree on. Assigned
        # directly (not split) so embedded spaces survive.
        cnt=0
        vals[++cnt]="\"\""
        vals[++cnt]="\"a\""
        vals[++cnt]="\"Hello\""
        vals[++cnt]="\"Hello World\""
        vals[++cnt]="\"MiXeD\""
        vals[++cnt]="\"123abc\""
        vals[++cnt]="\"end.\""
        vals[++cnt]="\"oo\""
        vals[++cnt]="\"o\""
      }
      for (i=1;i<=cnt;i++) {
        e = expr
        gsub(/A/, vals[i], e)
        if (arity == 2) {
          b = vals[((i * 7) % cnt) + 1]   # deterministic second operand
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

# nan_case NAME EXPR EXPECT
#   A single fixed NaN/inf-producing f64 expression that BOTH engines must print
#   identically (the to_str %g contract — sign-of-NaN and ±inf). Asserts the
#   exact string against EXPECT too, pinning the canonical form. (compare:
#   Stdout — the boundary the slice-2 random f64 domain deliberately avoided.)
nan_case() {
  local name="$1" expr="$2" expect="$3"
  local src="$WORK/$name.ax"
  printf 'fn main() -> i64 {\n    println(to_str(%s))\n    0\n}\n' "$expr" > "$src"
  local i_out n_out
  i_out="$("$AXON" run "$src" 2>/dev/null)"
  if ! "$AXON" build "$src" -o "$WORK/$name.bin" --no-cache >/dev/null 2>&1; then
    echo "  FAIL $name: native build failed"; fail=1; return
  fi
  n_out="$("$WORK/$name.bin" 2>/dev/null)"
  if [ "$i_out" != "$n_out" ]; then
    echo "  FAIL $name: interp='$i_out' native='$n_out' (NaN/inf format divergence)"; fail=1; return
  fi
  if [ "$i_out" != "$expect" ]; then
    echo "  FAIL $name: both engines agree on '$i_out' but expected '$expect'"; fail=1; return
  fi
  echo "  OK   $name: interp==native=='$i_out'"
}

# expect_overflow NAME EXPR
#   An i64 expression that OVERFLOWS. This is a KNOWN, documented divergence, NOT
#   an equality case (compare: ExitCode): the interpreter uses checked arithmetic
#   and aborts gracefully (non-zero exit, no stdout), while native codegen emits
#   wrapping two's-complement IR (exit 0, a wrapped value). The descriptor
#   asserts exactly that CONTRACT — interp aborts non-zero, native does not —
#   so a regression either way (interp silently wrapping, or native starting to
#   abort) is caught, without pretending the two agree.
expect_overflow() {
  local name="$1" expr="$2"
  local src="$WORK/$name.ax"
  printf 'fn main() -> i64 {\n    println(to_str(%s))\n    0\n}\n' "$expr" > "$src"
  local i_out i_exit n_exit
  i_out="$("$AXON" run "$src" 2>/dev/null)"; i_exit=$?
  if ! "$AXON" build "$src" -o "$WORK/$name.bin" --no-cache >/dev/null 2>&1; then
    echo "  FAIL $name: native build failed"; fail=1; return
  fi
  "$WORK/$name.bin" >/dev/null 2>&1; n_exit=$?
  # Contract: interp aborts (non-zero) on the overflow; stdout empty.
  if [ "$i_exit" = 0 ]; then
    echo "  FAIL $name: interp did NOT abort on overflow (exit 0, out='$i_out') — checked-arith regression?"; fail=1; return
  fi
  if [ -n "$i_out" ]; then
    echo "  FAIL $name: interp printed '$i_out' before aborting — expected no output"; fail=1; return
  fi
  echo "  OK   $name: interp aborts (exit $i_exit), native wraps (exit $n_exit) — documented i64-overflow divergence"
}

echo "fuzz_parity: seed=$SEED, up to $((N)) random + edge inputs per builtin"
# ── i64 scalars (extern + inline) ─────────────────────────────────────────────
fuzz abs_i64   i64 1 'abs_i64(A)'
fuzz abs_i32   i64 1 'abs_i32(A)'
fuzz sign_i64  i64 1 'sign_i64(A)'
fuzz min_i64   i64 2 'min_i64(A, B)'
fuzz max_i64   i64 2 'max_i64(A, B)'
fuzz min_i32   i64 2 'min_i32(A, B)'
fuzz max_i32   i64 2 'max_i32(A, B)'
fuzz clamp_i64 i64 1 'clamp_i64(A, 0 - 500, 500)'
# ── i64 binops + bitwise (inline emit_binop / emit_call) ──────────────────────
fuzz add_i64   i64 2 'A + B'
fuzz sub_i64   i64 2 'A - B'
fuzz cmp_lt    i64 2 'A < B'
fuzz cmp_eq    i64 2 'A == B'
fuzz bit_and   i64 2 'bit_and(A, B)'
fuzz bit_or    i64 2 'bit_or(A, B)'
fuzz bit_xor   i64 2 'bit_xor(A, B)'
fuzz bit_not   i64 1 'bit_not(A)'
# ── pos-domain: exponent / shift counts must stay in range ────────────────────
fuzz pow_i64   pos 2 'pow_i64(A % 10, B % 8)'
fuzz shl_i64   pos 2 'shl(A % 1000, B % 31)'
fuzz shr_i64   pos 2 'shr(A, B % 31)'
fuzz mod_i64   pos 2 'A % (B % 1000 + 1)'
# ── f64 math (printed through the shared %.6g to_str contract) ────────────────
fuzz abs_f64   f64 1 'abs_f64(A)'
fuzz floor_f64 f64 1 'floor(A)'
fuzz ceil_f64  f64 1 'ceil(A)'
fuzz min_f64   f64 2 'min_f64(A, B)'
fuzz max_f64   f64 2 'max_f64(A, B)'
fuzz f2i       f64 1 'f64_to_i64(A)'
# transcendentals (LLVM intrinsics → libm; native==interp). Domains kept finite:
# ln/log10 need a positive arg (abs+1 ≥ 1, avoids NaN — that path's its own
# nan_case); exp's arg is bounded small so it can't overflow to inf.
fuzz exp_f64   f64 1 'exp(min_f64(abs_f64(A), 700.0))'
fuzz ln_f64    f64 1 'ln(abs_f64(A) + 1.0)'
fuzz log10_f64 f64 1 'log10(abs_f64(A) + 1.0)'
# ── str scalars (str → i64/bool/str) ──────────────────────────────────────────
fuzz str_len      str 1 'str_len(A)'
fuzz str_contains str 2 'str_contains(A, B)'
fuzz str_starts   str 2 'str_starts_with(A, B)'
fuzz str_ends     str 2 'str_ends_with(A, B)'
fuzz str_index    str 2 'str_index_of(A, B)'
# ── slice 2b: f64 NaN/inf boundary (compare: Stdout — exact canonical form) ───
#   sqrt(-1) yields a NEGATIVE NaN; native snprintf would print "-nan" without
#   the to_str_f64 NaN-normalization (the fix that lands with this slice).
nan_case nan_sqrt_neg 'sqrt(0.0 - 1.0)'        'nan'
nan_case nan_zero_div '0.0 / 0.0'              'nan'
nan_case inf_pos      '1.0 / 0.0'              'inf'
nan_case inf_neg      '(0.0 - 1.0) / 0.0'      '-inf'
# ── slice 2b: i64 overflow boundary (compare: ExitCode — documented divergence)
expect_overflow ovf_add  '9223372036854775807 + 1'
expect_overflow ovf_sub  '(0 - 9223372036854775807 - 1) - 1'
expect_overflow ovf_mul  '9223372036854775807 * 2'
expect_overflow ovf_neg  '0 - (0 - 9223372036854775807 - 1)'

[ "$fail" -eq 0 ] || { echo "fuzz_parity: FAIL — interp↔codegen divergence found"; exit 1; }
echo "fuzz_parity: PASS — 33 random + 4 NaN/inf + 4 overflow-boundary descriptors agree with the documented I-2 contract ✓"
exit 0
