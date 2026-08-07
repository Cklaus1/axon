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

echo "5. VALUES persist, and their computation does not repeat"
SESS2="$(dirname "$SESS")/valsession.ax"
python3 "$ROOT/scripts/axon_session.py" new "$SESS2"
printf 'fn expensive() -> i64 { println("EXPENSIVE RAN")  42 }\nlet v = expensive()\n' > "$S/v1.ax"
V1="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS2" "$S/v1.ax" 2>&1)"
echo "$V1" | grep -q "EXPENSIVE RAN" && ok "cell 1 computed the value" \
  || bad "cell 1 did not run the computation: $V1"
printf 'println(to_str(v + 1))\n' > "$S/v2.ax"
V2="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS2" "$S/v2.ax" 2>&1)"
[ "$(echo "$V2" | tail -1)" = "43" ] && ok "cell 2 read the persisted VALUE" \
  || bad "value did not persist: $V2"
if echo "$V2" | grep -q "EXPENSIVE RAN"; then
  bad "the computation re-ran — values are being recomputed, not persisted"
else
  ok "the computation did not repeat"
fi

echo "6. read is a LOOKUP: it returns the value and executes nothing"
R="$(python3 "$ROOT/scripts/axon_session.py" read "$SESS2" v 2>&1)"
[ "$R" = "42" ] && ok "read returned the stored value" || bad "read returned: $R"
if echo "$R" | grep -q "EXPENSIVE RAN"; then
  bad "read executed the binding — the declarations-only blocker is back"
else
  ok "read fired no side effect (Engine::read is now honest)"
fi

echo "7. a binding with no literal form is REPORTED, not silently dropped"
printf 'let f = |x: i64| x + 1\nlet keep = 5\n' > "$S/v3.ax"
python3 "$ROOT/scripts/axon_session.py" eval "$SESS2" "$S/v3.ax" >/dev/null 2>&1
SHOWN="$(python3 "$ROOT/scripts/axon_session.py" show "$SESS2" 2>&1)"
if echo "$SHOWN" | grep -q "SKIPPED f"; then
  ok "unserializable binding reported in the skip list"
elif echo "$SHOWN" | grep -q "let keep = 5"; then
  bad "a binding vanished with no skip note: $SHOWN"
else
  bad "neither skipped nor kept — the skip path was not exercised: $SHOWN"
fi

echo "8. a MULTI-LINE let is not torn apart (models write these constantly)"
SESS3="$(dirname "$SESS")/multiline.ax"
python3 "$ROOT/scripts/axon_session.py" new "$SESS3"
printf 'let f = |x: i64| {\n    x + 1\n}\nprintln(to_str(f(1)))\n' > "$S/m1.ax"
M1OUT="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS3" "$S/m1.ax" 2>&1)"
[ "$(echo "$M1OUT" | tail -1)" = "2" ] && ok "multi-line let evaluated" \
  || bad "multi-line let was torn apart: $M1OUT"

echo "9. a value with braces does not wedge the session (M1)"
SESS4="$(dirname "$SESS")/braces.ax"
python3 "$ROOT/scripts/axon_session.py" new "$SESS4"
printf 'let j = chr(123)\n' > "$S/b1.ax"
python3 "$ROOT/scripts/axon_session.py" eval "$SESS4" "$S/b1.ax" >/dev/null 2>&1
printf 'println("still alive")\n' > "$S/b2.ax"
B2="$(python3 "$ROOT/scripts/axon_session.py" eval "$SESS4" "$S/b2.ax" 2>&1)"
[ "$(echo "$B2" | tail -1)" = "still alive" ] && ok "session survived a braced value" \
  || bad "a single brace bricked the session: $B2"

[ "$FAIL" = "0" ] && echo "axon_session_demo: PASS — all properties hold" \
  || echo "axon_session_demo: FAIL"
exit "$FAIL"
