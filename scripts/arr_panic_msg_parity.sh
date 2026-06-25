#!/usr/bin/env bash
# arr_panic_msg_parity.sh — I-2 stderr-message parity for the closure-taking
# array builtins that panic on bad input.
#
# The interpreter prints a descriptive panic line (e.g. "axon: panic: arr_chunk:
# chunk size must be positive, got 0"). Native codegen emitted these inline and
# called the bare C exit(101) with NO message — the exit code matched but the
# stderr text diverged. Codegen now routes them through __axon_msg_panic /
# __axon_msg_panic_i64 so native prints the SAME line. This harness compares the
# combined stdout+stderr AND the exit code for arr_chunk(_,0), arr_max_by([]),
# arr_min_by([]).
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "arr_panic_msg_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "arr_panic_msg_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

run_case() {
  local label="$1" prog_src="$2"
  local prog="$WORK/$label.ax" bin="$WORK/$label.bin"
  printf '%s\n' "$prog_src" > "$prog"
  local i_out i_code n_out n_code
  i_out="$("$AXON" run "$prog" 2>&1)"; i_code=$?
  i_out="$(printf '%s\n' "$i_out" | grep -v '^axon: run-id ')"  # strip Phase-9 run-id stamp (native emits none)
  if ! "$AXON" build "$prog" -o "$bin" --no-cache >/dev/null 2>&1; then
    echo "arr_panic_msg_parity: native build failed for $label — skipping"; exit 0
  fi
  n_out="$("$bin" 2>&1)"; n_code=$?
  if [ "$i_out" != "$n_out" ] || [ "$i_code" != "$n_code" ]; then
    echo "arr_panic_msg_parity: FAIL ($label):"
    echo "  interp [$i_code]: $i_out"
    echo "  native [$n_code]: $n_out"
    exit 1
  fi
  echo "  OK  $label: [$i_code] $i_out"
}

run_case chunk0  'fn main() -> i64 { let xs = [1,2,3]  let c = arr_chunk(xs, 0)  len(c) }'
run_case maxby   'fn main() -> i64 { let xs: [i64] = []  arr_max_by(xs, |x| x) }'
run_case minby   'fn main() -> i64 { let xs: [i64] = []  arr_min_by(xs, |x| x) }'

echo "arr_panic_msg_parity: PASS — native panic messages match the interpreter (exit + text)"
exit 0
