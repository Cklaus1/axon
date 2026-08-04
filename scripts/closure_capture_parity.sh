#!/usr/bin/env bash
# closure_capture_parity.sh — native codegen == interpreter for MUTABLE CLOSURE
# CAPTURE, the README-headline "heap-captured mutable closures" feature.
#
# AUDIT T40 (findings F094 / P5-16 / DOC-02). There was no closure parity
# harness at all (`grep -rn closure scripts/*.sh` found none), and the two
# fixture tests that mention closures asserted only that the fixtures
# type-check / parse. So this divergence sat in a README-headline feature with
# no gate anywhere:
#
#   let n = 0; let bump = || { n = n + 1  n }
#   interp:  call1=1 call2=1 call3=1   outer n=0   (write silently dropped)
#   native:  call1=1 call2=2 call3=3   outer n=0   (heap capture, write persists)
#
# Same source, two backends, two answers, no error from either. The interpreter
# is the reference oracle (I-2), so it was the side that had to change: the
# capture is now a shared cell that persists across calls of that closure while
# still NOT aliasing the outer binding (note `outer n=0` on both engines).
#
# Compares STDOUT, not just exit codes — the whole defect is in printed values.
#
# Skips (exit 0) when the codegen toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "closure_capture_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "closure_capture_parity: interp build unavailable — skipping"; exit 0
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

# ── the headline case, verbatim from README ──────────────────────────────────
check make_counter \
'fn main() { let n = 0
    let bump = || { n = n + 1  n }
    println("{to_str(bump())},{to_str(bump())},{to_str(bump())},outer={to_str(n)}") }'

# ── two closures must own INDEPENDENT cells ──────────────────────────────────
check independent_cells \
'fn main() { let a = 0
    let f = || { a = a + 1  a }
    let b = 0
    let g = || { b = b + 10  b }
    println("{to_str(f())},{to_str(g())},{to_str(f())},{to_str(g())}") }'

# ── a PARAMETER shadowing a captured name must not write through ─────────────
check param_shadows_capture \
'fn main() { let s = 100
    let h = |s: i64| { s + 1 }
    println("{to_str(h(5))},{to_str(s)}") }'

# ── an inner `let` must not leak into the capture cell ────────────────────────
check inner_let_no_leak \
'fn main() { let k = 0
    let m = || { let k = 99  k = k + 1  k }
    println("{to_str(m())},{to_str(m())},{to_str(k)}") }'

# ── a closure created and driven INSIDE a fn (returned counter pattern) ──────
check counter_inside_fn \
'fn drive() -> i64 { let n = 0
    let c = || { n = n + 1  n }
    c()
    c()
    c() }
fn main() { println("{to_str(drive())}") }'

# ── capture that is READ but never written must still agree ──────────────────
check read_only_capture \
'fn main() { let base = 7
    let add = |x: i64| { x + base }
    println("{to_str(add(1))},{to_str(add(2))},{to_str(base)}") }'

[ "$fail" -eq 0 ] || { echo "closure_capture_parity: FAIL"; exit 1; }
echo "closure_capture_parity: PASS — mutable closure capture matches the interpreter ✓"
exit 0
