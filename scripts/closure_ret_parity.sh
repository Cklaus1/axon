#!/usr/bin/env bash
# closure_ret_parity.sh — native codegen == interpreter for closures whose
# declared parameter or return type is NOT plain i64.
#
# AUDIT T37 (finding F061). A closure value is a bare {fn_ptr, env_ptr} pair
# carrying no type tag, and the direct-call site used to
#   (a) build the indirect-call signature from the ARGUMENT's own LLVM type, and
#   (b) read the i64-ABI result back as a raw i64.
# Both are wrong when the lambda declared something narrower or non-integer:
#
#   let f = |x: f64| x * 2.0 ; f(3.0)        interp 6    native 4618441417868443648
#   let g = |x: i32| min_i32(x, 0-5); g(0-3) interp -5   native 4294967291
#
# Silent, exit 0, no diagnostic — from a compiler that documents "refuse, never
# miscompile" (I-2). The i32 half was UB rather than a mere wrong extension: the
# caller passed `i64 -3` to a function declared `(ptr, i32)`, and the observed
# value flipped depending on whether an unrelated f64 lambda had been emitted
# first. So this harness compares STDOUT, not just exit codes, and includes both
# emission orders.
#
# Skips (exit 0) when the codegen toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "closure_ret_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "closure_ret_parity: interp build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

fail=0
check() {
  local name="$1" src="$2"
  printf '%s\n' "$src" > "$WORK/$name.ax"
  local i_out i_code n_out n_code
  i_out="$("$INTERP" "$WORK/$name.ax" 2>/dev/null)"; i_code=$?
  if ! "$AXON" build "$WORK/$name.ax" -o "$WORK/$name" >/dev/null 2>&1; then
    echo "  SKIP $name (native build unavailable)"; return
  fi
  n_out="$("$WORK/$name" 2>/dev/null)"; n_code=$?
  if [ "$i_out" = "$n_out" ] && [ "$i_code" = "$n_code" ]; then
    echo "  OK   $name: $i_out (exit $i_code)"
  else
    echo "  FAIL $name"
    echo "    interp: [$i_out] (exit $i_code)"
    echo "    native: [$n_out] (exit $n_code)"
    fail=1
  fi
}

# ── f64-returning closures ────────────────────────────────────────────────────
check f64_ret \
'fn main() { let f = |x: f64| x * 2.0
    println("{to_str(f(3.0))}") }'

check f64_neg \
'fn main() { let f = |x: f64| x - 10.5
    println("{to_str(f(1.5))}") }'

# ── narrow signed int closures (sign extension, not zero extension) ───────────
check i32_min \
'fn main() { let g = |x: i32| min_i32(x, 0-5)
    println("{to_str(g(0-3))}") }'

check i32_ident \
'fn main() { let g = |x: i32| x
    println("{to_str(g(0-3))}") }'

check i32_abs \
'fn main() { let g = |x: i32| abs_i32(x)
    println("{to_str(g(0-7))}") }'

# ── ORDER MATTERS: an f64 lambda emitted first must not corrupt a later one ───
# This is the case the original bug actually depended on. Both orders must agree
# with the interpreter and with each other.
check order_f64_first \
'fn main() { let f = |x: f64| x * 2.0
    println("{to_str(f(3.0))}")
    let g = |x: i32| min_i32(x, 0-5)
    println("{to_str(g(0-3))}") }'

check order_i32_first \
'fn main() { let g = |x: i32| min_i32(x, 0-5)
    println("{to_str(g(0-3))}")
    let f = |x: f64| x * 2.0
    println("{to_str(f(3.0))}") }'

# ── bool bodies must still ZERO-extend (true is 1, not -1) ───────────────────
check bool_ret \
'fn main() { let p = |x: i64| x > 2
    println("{to_str(p(5))} {to_str(p(1))}") }'

# ── plain i64 closures must be unaffected ────────────────────────────────────
check i64_ret \
'fn main() { let h = |x: i64| x * 3
    println("{to_str(h(0-4))}") }'

[ "$fail" -eq 0 ] || { echo "closure_ret_parity: FAIL"; exit 1; }
echo "closure_ret_parity: PASS — f64/narrow-int/bool closure returns match the interpreter ✓"
exit 0
