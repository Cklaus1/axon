#!/usr/bin/env bash
# SPIKE demo/self-test — `AXON_FOR_RLM.md` §5, declarations-only session.
#
# Asserts the four properties the spike exists to establish. Run it; a silent
# exit 0 means all four hold.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SESS="$(mktemp -d)/session.ax"
S="$(dirname "$SESS")"
FAIL=0
ok()   { echo "  PASS: $1"; }
bad()  { echo "  FAIL: $1"; FAIL=1; }

if [ ! -x "$ROOT/target/debug/axon" ]; then
  echo "SKIP: no axon binary (cargo build -p axon-core --bin axon)"; exit 0
fi

python3 "$ROOT/scripts/axon_session.py" new "$SESS"

echo "1. the defining RLM property: bind a name in one call, read it in the next"
printf 'fn rows() -> [i64] { [10, 20, 30] }\nprintln("cell 1 ran")\n' > "$S/c1.ax"
OUT1="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS" "$S/c1.ax" 2>&1)"
printf 'let v = rows()\nprintln(to_str(v[1]))\n' > "$S/c2.ax"
OUT2="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS" "$S/c2.ax" 2>&1)"
[ "$(echo "$OUT2" | tail -1)" = "20" ] && ok "cell 2 read cell 1's binding" \
  || bad "cell 2 could not read cell 1's binding: $OUT2"

echo "2. no side-effect replay: a prior cell's output must not re-run"
if echo "$OUT2" | grep -q "cell 1 ran"; then
  bad "cell 1's println re-ran in cell 2 — prior cells are re-executing"
else
  ok "cell 1's side effect did not repeat"
fi

echo "3. THE DIFFERENTIATOR: blind reuse at the wrong type is refused BEFORE running"
printf 'println(str_to_upper(rows()))\nprintln("THIS MUST NOT PRINT")\n' > "$S/c3.ax"
OUT3="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS" "$S/c3.ax" 2>&1)"; RC3=$?
if [ "$RC3" = "2" ] && echo "$OUT3" | grep -q "found.*\[i64\]"; then
  ok "refused, naming the real type (a REPL learns this only at runtime)"
else
  bad "wrong-type reuse was not refused with a type: rc=$RC3 $OUT3"
fi
if echo "$OUT3" | grep -q "THIS MUST NOT PRINT"; then
  bad "the cell EXECUTED before being type-checked"
else
  ok "nothing executed"
fi

echo "4. a refused cell leaves the session usable"
printf 'println(to_str(rows()[0]))\n' > "$S/c4.ax"
OUT4="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS" "$S/c4.ax" 2>&1)"
[ "$(echo "$OUT4" | tail -1)" = "10" ] && ok "session intact after a refusal" \
  || bad "session corrupted by the refused cell: $OUT4"

[ "$FAIL" = "0" ] && echo "axon_session_demo: PASS — all four properties hold" \
  || echo "axon_session_demo: FAIL"
exit "$FAIL"
