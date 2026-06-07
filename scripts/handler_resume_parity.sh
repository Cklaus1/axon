#!/usr/bin/env bash
# handler_resume_parity.sh — the LOWERED effect-handler subset must run
# native == interpreter, byte for byte (I-2).
#
# Native codegen lowers the narrow subset of `with handler { … } { body }` that
# is an inline, tail-resumptive (`on E(p) => resume(v)`) handler intercepting a
# DIRECT builtin. `resume(v)` becomes the intercepted operation's result and the
# body continues — straight-line IR, no continuation runtime. Everything outside
# the subset (indirect/closure/method interception, abort arms, return arms)
# stays E0910-refused, so it can never miscompile. This harness builds each
# lowered-subset program both ways and diffs (exit code, stdout).
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "handler_resume_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "handler_resume_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

fail=0
check() {
  local name="$1" src="$2"
  local prog="$WORK/$name.ax"
  printf '%b\n' "$src" > "$prog"

  AXON_AI_MOCK=1 AXON_SEED=42 "$AXON" run "$prog" >"$WORK/$name.iout" 2>/dev/null
  local i_exit=$?

  local bin="$WORK/${name}_bin"
  local berr
  berr="$(AXON_AI_MOCK=1 "$AXON" build "$prog" -o "$bin" --no-cache 2>&1)"
  if echo "$berr" | grep -q "E0910"; then
    echo "FAIL [$name]: this shape should LOWER but was E0910-refused"
    fail=1
    return
  fi
  if [ ! -x "$bin" ]; then
    echo "handler_resume_parity: native build/link unavailable for $name — skipping"
    echo "  $berr" | head -2
    exit 0
  fi
  AXON_AI_MOCK=1 AXON_SEED=42 "$bin" >"$WORK/$name.nout" 2>/dev/null
  local n_exit=$?

  if [ "$i_exit" != "$n_exit" ]; then
    echo "FAIL [$name]: exit interp=$i_exit native=$n_exit"
    fail=1
  elif ! diff -q "$WORK/$name.iout" "$WORK/$name.nout" >/dev/null; then
    echo "FAIL [$name]: stdout diverges"
    diff "$WORK/$name.iout" "$WORK/$name.nout" | head
    fail=1
  else
    echo "  OK $name: native==interp (exit $i_exit)"
  fi
}

# resume substitutes a value-returning builtin's result.
check resume_value 'fn main() -> i64 { with handler { on Random(p) => resume(7) } { random_i64(0, 9) } }'
# resume value flows through the surrounding expression (mid-expression).
check resume_midexpr 'fn main() -> i64 { with handler { on Random(p) => resume(10) } { random_i64(0, 5) * 4 + 1 } }'
# IO suppressed; the block tail is the value.
check io_suppress 'fn main() -> i64 { with handler { on IO(p) => resume(0) } { println("NO")\n 5 } }'

# An arm that REFERENCES the payload binding `p` must NOT be lowered (codegen
# does not bind the payload) — it stays E0910-refused. Verify it is refused
# rather than silently built (a built one would diverge from the interpreter).
refused() {
  local name="$1" src="$2"
  local prog="$WORK/$name.ax"
  printf '%b\n' "$src" > "$prog"
  if AXON_AI_MOCK=1 "$AXON" build "$prog" -o "$WORK/${name}_bin" --no-cache 2>&1 | grep -q "E0910"; then
    echo "  OK $name: correctly E0910-refused (not lowered)"
  else
    echo "FAIL [$name]: a payload-referencing arm must be refused, not lowered"
    fail=1
  fi
}
refused payload_ref 'fn main() -> i64 { with handler { on Random(p) => resume(p + 100) } { random_i64(0, 9) } }'

if [ "$fail" -ne 0 ]; then
  echo "handler_resume_parity: FAIL — lowered handler diverges from interp"
  exit 1
fi
echo "handler_resume_parity: native==interp on all lowered cases"
echo "handler_resume_parity: PASS"
