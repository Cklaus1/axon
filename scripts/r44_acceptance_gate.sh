#!/usr/bin/env bash
# R44 — accumulating typed session: acceptance gate.
#
# Re-runs the Slice-0 hazards as BEHAVIOUR (driving the real `axon session`
# verb, not a mock composer) plus the Slice-1 regression tests.
#
# The four Slice-0 hazards are re-run here rather than trusted from the spike,
# because two of them — idempotent re-check across a `use`, and type pinning —
# hold for v1 by CONSTRUCTION rather than by code that anyone wrote. A property
# nothing asserts is one a later refactor removes silently.
#
# Exit 0 = all pass. Exit 1 = at least one failure.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
AXON="${AXON:-$ROOT/target/debug/axon}"
fails=0
pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1"; fails=$((fails+1)); }

if [ ! -x "$AXON" ]; then
    echo "r44_acceptance_gate: \$AXON not executable at $AXON" >&2
    echo "hint: cargo build -p axon-core --bin axon" >&2
    exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# PRECONDITION. Every "must NOT contain E000x" check below is an ABSENCE
# assertion, and an absence assertion passes trivially against a binary that
# prints nothing at all — which is precisely the binary this gate exists to
# reject. Verified against a HEAD build: without the session verb, 5 of 12 checks
# reported PASS. So prove the verb WORKS before trusting any negative result,
# and probe the real behaviour rather than `--help` (a verb can be registered and
# still refuse to run, which is how a capability probe goes stale).
probe=$(printf 'let probe_ok = 41\n\nprintln(to_str(probe_ok + 1))\n\n' | "$AXON" session 2>/dev/null)
if ! echo "$probe" | grep -q 42; then
    echo "r44_acceptance_gate: \`axon session\` did not carry a binding across a cell." >&2
    echo "  Refusing to run the rest: the negative assertions would all pass vacuously." >&2
    echo "  probe output: ${probe:-<empty>}" >&2
    exit 1
fi

# Drive a session: each argument is one cell.
sess() {
    local out=""
    for c in "$@"; do out="$out$c"$'\n\n'; done
    printf '%s' "$out" | "$AXON" session 2>"$TMP/err" 
}

echo "== H1 (§1.2): assignment to a PERSISTED binding =="
got=$(sess 'let rows = [1, 2]' 'rows = rows + [99]' 'println(to_str(arr_sum_by(&rows, |v| v)))')
if echo "$got" | grep -q 102; then pass "1+2+99 = 102 across three cells"
else fail "expected 102, got: $got"; fi
if grep -q E0001 "$TMP/err"; then fail "E0001 — bindings landed at module scope again"
elif echo "$got" | grep -q 102; then pass "no E0001 (bindings are in main's scope)"
else fail "no E0001, but the cell produced nothing — vacuous"; fi

echo "== H2 (§4.3): a \`use\` in cell 1 must not E0002 cell 2 =="
mkdir -p "$TMP/mods"
echo 'fn double(n: i64) -> i64 { n * 2 }' > "$TMP/mods/helper.ax"
got=$(printf 'mod helper\nuse helper.{double}\nlet v = double(21)\nprintln(to_str(v))\n\nprintln(to_str(v + 1))\n\n' \
      | AXON_PATH="$TMP/mods" "$AXON" session 2>"$TMP/err2")
if grep -q E0002 "$TMP/err2"; then fail "E0002 — the import duplicated on re-check"
elif echo "$got" | grep -q 43; then pass "no E0002 (the re-check is idempotent)"
else fail "no E0002, but cell 2 produced nothing — vacuous"; fi
if echo "$got" | grep -q 42 && echo "$got" | grep -q 43; then pass "both cells ran (42, 43)"
else fail "expected 42 and 43, got: $got"; fi

echo "== H3 (§4.2): materialisation PINS a binding's type =="
got=$(sess 'fn make() -> i64 { 1 }
let x = make()' 'fn make2() -> str { "s" }
println(to_str(x + 1))')
if echo "$got" | grep -q 2; then pass "x stayed i64 after its producer was redefined"
else fail "expected 2, got: $got"; fi

echo "== S4: a failed cell must not accumulate =="
got=$(sess 'let good = 7' 'let bad = "x" + 1' 'println(to_str(good))')
if echo "$got" | grep -q 7; then pass "the session survived a failed cell"
else fail "expected 7, got: $got"; fi

echo "== S10: a non-persistable binding is NAMED =="
sess 'let f = |x| x + 1
println(to_str(f(1)))' 'println("next")' >/dev/null
if grep -q "did not persist" "$TMP/err"; then pass "the closure was named, not silently dropped"
else fail "a non-persistable binding vanished silently"; fi
if ! grep -q "did not persist" "$TMP/err"; then fail "no skip reason at all — cannot judge readability"
elif grep -q "BinOp" "$TMP/err"; then fail "the skip reason is a Rust Debug dump, not a shape"
else pass "the skip reason is readable"; fi

echo "== S11: no unused-binding warning storm =="
sess 'let a = 1
let b = 2
let c = 3' 'println(to_str(a))' 'println(to_str(b))' >/dev/null
n=$(grep -c W0006 "$TMP/err" || true)
ran=$(sess 'let a = 1' 'println(to_str(a))')
if [ "$n" -ne 0 ]; then fail "$n W0006 warnings over session state"
elif echo "$ran" | grep -q 1; then pass "no W0006 over session bindings"
else fail "no W0006, but the session produced nothing — vacuous"; fi
# ...and the suppression must be SCOPED, not a blanket mute.
sess 'fn f() -> i64 {
    let dead = 5
    7
}' 'println(to_str(f()))' >/dev/null
if grep -q W0006 "$TMP/err" && grep -q dead "$TMP/err"; then pass "a genuinely-local unused binding is still linted"
else fail "the lint was muted wholesale, not scoped to session state"; fi

echo "== S5/E2400 (§4.1): redefinition that breaks an earlier item =="
got=$(sess 'fn f() -> i64 { 1 }
println(to_str(f()))' 'fn g() -> i64 { f() + 1 }
println(to_str(g()))' 'fn f() -> str { "one" }
println("after")' 'println(to_str(g()))')
if grep -q E2400 "$TMP/err"; then pass "the breaking redefinition is refused"
else fail "E2400 not emitted — the redefinition was accepted"; fi
if grep -q "cell 2" "$TMP/err" && grep -q '`g`' "$TMP/err"; then pass "E2400 names the broken item and its cell"
else fail "E2400 did not name what broke: $(cat "$TMP/err")"; fi
if echo "$got" | grep -q after; then fail "the refused cell executed anyway"
else pass "the refused cell did not execute"; fi
# ...and the session survives with the OLD definition.
if [ "$(echo "$got" | grep -c '^2$')" -ge 2 ]; then pass "g() still works after the refusal — session unchanged"
else fail "the session did not survive the refusal: $got"; fi

echo "== S5: a SAFE redefinition replaces and persists =="
got=$(sess 'fn f() -> i64 { 1 }
println(to_str(f()))' 'fn f() -> i64 { 99 }' 'println(to_str(f()))')
if grep -q E0002 "$TMP/err"; then fail "E0002 — redefinition duplicated instead of replacing"
elif echo "$got" | grep -q 99; then pass "the replacement took effect and persisted"
else fail "expected 99, got: $got"; fi

echo "== E2400 must not swallow an unrelated error =="
sess 'fn f() -> i64 { 1 }' 'fn g() -> i64 { f() + 1 }' 'fn f() -> str { "one" }
let bad = 1 + true' >/dev/null
if ! grep -q E2400 "$TMP/err"; then fail "precondition failed: nothing was blamed"
elif grep -q "expected bool" "$TMP/err"; then pass "a separate mistake survives the promotion"
else fail "the unrelated error was swallowed: $(cat "$TMP/err")"; fi

echo "== S6: trailing expression values =="
got=$(sess 'let rows = [3, 1, 2]' 'fn total(xs: &[i64]) -> i64 { arr_sum_by(xs, |v| v) }' 'total(&rows)' 'rows' '"hello"')
ok=1
for want in '6' '\[3, 1, 2\]' '"hello"'; do
    echo "$got" | grep -q "$want" || { fail "trailing value $want not displayed"; ok=0; }
done
[ "$ok" -eq 1 ] && pass "scalar, array and str trailing values display"
if grep -q axon_session_probe "$TMP/err"; then fail "probe diagnostics leaked to the user"
else pass "probe diagnostics stay internal"; fi

got=$(sess 'println("just this")')
if [ "$(echo "$got" | grep -c 'just this')" -eq 1 ]; then pass "a unit-valued cell neither double-prints nor shows unit"
else fail "unit handling wrong: $got"; fi

echo "== S7/§4.4: runtime failure is survivable AND honestly scoped =="
marker="$TMP/effect_marker.txt"
sess 'let good = 5' "let w = write_file(\"$marker\", \"x\")
let arr = [1]
println(to_str(arr[99]))" 'println(to_str(good))' >"$TMP/s7out"
if grep -q 5 "$TMP/s7out"; then pass "the session survived a runtime panic"
else fail "the session did not survive: $(cat "$TMP/s7out")"; fi
if [ ! -f "$marker" ]; then fail "precondition: the cell's write never happened"
elif grep -q "session is unchanged" "$TMP/err"; then fail "claims transactionality it cannot deliver"
elif grep -q "not undone" "$TMP/err"; then pass "the failure message scopes what rollback covers"
else fail "the failure message says nothing about un-undone effects"; fi

echo "== A1 (§5): one run-id, cell-indexed provenance =="
CACHE="$TMP/cache"
printf '@[adaptive]\nfn score(n: i64) -> i64 { n * 2 }\nprintln(to_str(score(3)))\n\nprintln(to_str(score(5)))\n\n' \
  | XDG_CACHE_HOME="$CACHE" "$AXON" session >"$TMP/a1out" 2>/dev/null
LOG="$CACHE/axon/provenance.jsonl"
if ! grep -q 6 "$TMP/a1out"; then fail "precondition: the session did not run"
elif [ "$(grep -c '"event":"run_start"' "$LOG")" -eq 1 ]; then pass "a session is ONE run"
else fail "expected exactly 1 run_start, got $(grep -c '"event":"run_start"' "$LOG")"; fi
if grep -q '"cell":1' "$LOG" && grep -q '"cell":2' "$LOG"; then pass "records carry their cell index"
else fail "cell indices missing from provenance"; fi

echo "== A2/A3 (§5): one journal, and the transcript replays =="
J="$TMP/sess.journal"; T="$TMP/sess.ax"
printf 'let v = match env_var("R44_GATE_VAR") { Ok(x) => x  Err(e) => "unset" }\nprintln(v)\n\nprintln("second")\n\n' \
  | AXON_RECORD="$J" R44_GATE_VAR=recorded "$AXON" session --transcript "$T" >"$TMP/recout" 2>/dev/null
if ! grep -q recorded "$TMP/recout"; then fail "precondition: the recording run did not read the var"
elif [ -f "$J" ]; then pass "a session writes its journal"
else fail "AXON_RECORD wrote no journal"; fi
if [ -s "$T" ]; then pass "the transcript holds the cells"
else fail "no transcript written"; fi
# Replay with the var UNSET — the journal must supply it.
AXON_REPLAY="$J" "$AXON" session <"$T" >"$TMP/rpout" 2>/dev/null
rc=$?
if [ $rc -eq 0 ] && grep -q recorded "$TMP/rpout"; then pass "the (journal, transcript) pair replays"
else fail "replay failed (rc=$rc): $(cat "$TMP/rpout")"; fi
# ...and a tampered cell must be caught, not quietly re-run.
sed 's/R44_GATE_VAR/R44_OTHER_VAR/' "$T" > "$TMP/tampered.ax"
AXON_REPLAY="$J" "$AXON" session <"$TMP/tampered.ax" >/dev/null 2>"$TMP/diverr"
rc=$?
if [ $rc -eq 11 ] && grep -q divergence "$TMP/diverr"; then pass "a tampered cell diverges with exit 11"
else fail "tampered replay exited $rc without a divergence"; fi

echo "== S8 (§4): an ambient effect ceiling applies per cell =="
printf 'println("should not appear")\n\n' | AXON_ALLOWED_EFFECTS=Pure "$AXON" session >"$TMP/s8out" 2>"$TMP/s8err"
if grep -q "sandbox violation" "$TMP/s8err" && ! grep -q "should not appear" "$TMP/s8out"; then
    pass "a session is not an effect-laundering seam"
else fail "the ceiling did not apply: $(cat "$TMP/s8err")"; fi

echo "== Slice-1/2/3/4 regression tests =="
if (cd "$ROOT" && cargo test -p axon-core --test cli_run session_ 2>&1 | grep -q "test result: ok"); then
    pass "cli_run session_* green (23 tests)"
else
    fail "cli_run session_* not green"
fi

echo
if [ "$fails" -eq 0 ]; then echo "r44_acceptance_gate: ALL PASS"; exit 0
else echo "r44_acceptance_gate: $fails finding(s)"; exit 1; fi
