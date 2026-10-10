#!/usr/bin/env bash
# R51 smoke: sealed suite verdicts through the real CLI, as the axon-psv runner
# invokes it (`env -i`, AXON_PATH, `axon test --seal <cand> --json`).
#
# usage: run.sh <axon-binary> <phase>
#   phase=pre   (S0-S5): every sealed invocation is refused, exit 2, no summary.
#   phase=post  (S6+):   honest candidate passes; each attack is refused;
#                        AXON_ENGINE=vm is refused; no VM trace lines.
# The three attacks are refused by the sealed-only static check (operator names
# read as missing, E0001); the runtime edges behind it are covered by T3/T8
# with the static check bypassed. Fixture validated on v0.22 c9ed2ddd: honest
# and attack rows pass there (it has no VM, so the VM row fails there).
# Exit 0 iff every row meets its expectation. Prints one line per row.
set -u
AXON=$(readlink -f "$1"); PHASE=$2
W=$(mktemp -d)
if [ -n "${KEEP:-}" ]; then echo "work=$W" >&2; else trap 'rm -rf "$W"' EXIT; fi
fail=0

SUITE='mod sol
use sol.{solve}
type Cfg = { k: i64 }
let TABLE = [9, 8]
let CFG = Cfg { k: 9 }
let PAIR = (9, 4)
@[test]
fn accept() {
    assert_eq(solve(3), 4)
}
'
declare -A CAND=(
  [honest]='pub fn inc(n: i64) -> i64 { n + 1 }
pub fn solve(n: i64) -> i64 {
    let g = inc
    g(n)
}
'
  [attack_index]='pub fn solve(n: i64) -> i64 { TABLE[0] - 5 }
'
  [attack_field]='pub fn solve(n: i64) -> i64 { CFG.k - 5 }
'
  [attack_tuple]='pub fn solve(n: i64) -> i64 { PAIR.0 - 5 }
'
)

# run <name> [VAR=value ...] -> sets RC, OUT (stdout), ERR (stderr)
run() {
  local name=$1; shift
  local d="$W/$name"; mkdir -p "$d/cand" "$d/suite"
  printf '%s' "${CAND[${name%%+*}]}" > "$d/cand/sol.ax"
  printf '%s' "$SUITE" > "$d/suite/accept.ax"
  ( cd "$d/suite" && env -i PATH=/usr/bin:/bin \
      AXON_PATH="$d/suite:$d/cand" AXON_PATH_EXCLUSIVE=1 "$@" \
      timeout 60 "$AXON" test "$d/suite/accept.ax" --json --seal "$d/cand" \
      >"$d/out" 2>"$d/err" ); RC=$?
  OUT=$(cat "$d/out"); ERR=$(cat "$d/err")
}
summary() { grep -o '"type":"summary"[^}]*' <<<"$OUT" | head -1; }
row() { # row <label> <ok:0|1> <detail>
  if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1: $3"; fail=1; fi
}

if [ "$PHASE" = pre ]; then
  for c in honest attack_index; do
    for env in "" "AXON_ENGINE=vm"; do
      run "$c" $env
      ok=1
      [ "$RC" = 2 ] && grep -q 'R51 SEAL_RUNTIME_COMPLETE' <<<"$ERR" && [ -z "$(summary)" ] && ok=0
      row "pre $c ${env:-default}" $ok "rc=$RC err=$(head -c 300 <<<"$ERR") summary=$(summary)"
    done
  done
else
  run honest
  ok=1; [ "$RC" = 0 ] && grep -q '"passed":1' <<<"$(summary)" && ok=0
  row "honest passes" $ok "rc=$RC summary=$(summary) err=$(head -c 300 <<<"$ERR")"
  run honest AXON_VM_TRACE=1
  ok=1; [ "$RC" = 0 ] && ! grep -qi 'vm' <<<"$ERR" && ok=0
  row "honest runs on the tree (no VM trace)" $ok "err=$(head -c 300 <<<"$ERR")"
  run honest AXON_ENGINE=vm
  ok=1; [ "$RC" = 2 ] && grep -q 'tree engine only' <<<"$ERR" && [ -z "$(summary)" ] && ok=0
  row "AXON_ENGINE=vm refused" $ok "rc=$RC err=$(head -c 300 <<<"$ERR")"
  for c in attack_index attack_field attack_tuple; do
    run "$c"
    ok=1; ! grep -q '"passed":1' <<<"$(summary)" && [ "$RC" != 0 ] && ok=0
    row "$c refused" $ok "rc=$RC summary=$(summary) err=$(head -c 300 <<<"$ERR")"
  done
fi
exit $fail
