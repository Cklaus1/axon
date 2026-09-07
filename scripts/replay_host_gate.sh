#!/usr/bin/env bash
# replay_host_gate.sh — gate for AXON_RECORD / AXON_REPLAY (the host journal).
#
# WHY THIS HARNESS EXISTS.
#
# The claim is: a run whose only nondeterminism is the environment can be
# reproduced exactly, later, on a machine where that environment is GONE. That
# claim is only worth anything if the negative cases hold too — a replay that
# quietly consults live state produces an authoritative-looking transcript of a
# run that never happened, which is strictly worse for an auditor than having no
# replay at all.
#
# So this harness checks BOTH directions, and the negative direction is the point:
#
#   1. record        — a run touching the world writes a journal;
#   2. replay        — the SAME program reproduces its output byte-for-byte with
#                      the file DELETED and stdin CLOSED (the property);
#   3. control       — without the journal, that same environment produces
#                      DIFFERENT output. Without this check, 2 could be passing
#                      for the trivial reason that the program never needed the
#                      world at all;
#   4. divergence    — a program that asks for something else is refused, exit 11,
#                      and the report names the FIRST point of departure;
#   5. unswallowable — a program that CATCHES the host error and prints "all
#                      good" still exits 11. A guard the program under audit can
#                      suppress is not a guard;
#   6. short replay  — consuming LESS than the journal is also a divergence (it
#                      is invisible from inside a host call, so it is checked at
#                      exit);
#   7. no writes     — replaying a run that wrote a file must NOT write it. An
#                      audit that re-performs the effects it is studying is its
#                      own accident;
#   8. mutual excl.  — recording and replaying at once is refused, not silently
#                      resolved in some order nobody chose;
#   9. off by default— with neither variable set, behaviour is unchanged;
#  10-12. corrupt journal refused; capability gate not launderable;
#  13. transcript   — a PERSON can read it, and it redacts secrets by default;
#  14. diff         — names the first divergence and the axis, exit 11;
#  15. axon goal    — an optimizer's result is reproducible (with a control).
#
# This asserts EXPECTED VALUES, not merely "the two runs agree": an agreement
# oracle cannot see a bug that both the recorder and the replayer share, which is
# exactly the shape a shared encode/decode pair fails in.
#
# Exit 0 = pass. Exit 1 = a real failure. No codegen dependency — the journal is
# an interpreter-level facility.

set -uo pipefail
cd "$(dirname "$0")/.."

AXON="${AXON:-./target/debug/axon}"
REPO="$(pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ ! -x "$AXON" ]; then
  echo "replay_host_gate: SKIP — no axon binary at $AXON"
  exit 0
fi
AXON="$(cd "$(dirname "$AXON")" && pwd)/$(basename "$AXON")"

pass=0; fail=0
ok()  { echo "  OK $1"; pass=$((pass+1)); }
bad() { echo "FAIL [$1]: $2"; fail=$((fail+1)); }

J="$WORK/journal.txt"
cd "$WORK"

# The probe reads a file and a line of stdin — the two channels that were wholly
# unreproducible before this existed (stdin did not even reach the host seam).
cat > p.ax <<'AXEOF'
fn main() -> i64 {
    match read_file("./data.txt") {
        Ok(s) => println("file: {s}")
        Err(e) => println("nofile")
    }
    println("stdin: {read_line()}")
    0
}
AXEOF
printf 'the recorded contents' > data.txt

# ── 1: record ────────────────────────────────────────────────────────────────
printf 'recorded-input\n' | AXON_RECORD="$J" "$AXON" run p.ax > rec.out 2> rec.err
rec_code=$?
if [ $rec_code -ne 0 ]; then
  bad record "recording run exited $rec_code: $(cat rec.err)"
elif [ ! -s "$J" ]; then
  bad record "no journal was written to $J"
else
  ok "record: journal written ($(wc -l < "$J") events)"
fi

WANT="$(printf 'file: the recorded contents\nstdin: recorded-input\n')"
if [ "$(cat rec.out)" = "$WANT" ]; then
  ok "record: the recorded run produced the expected output"
else
  bad record_values "recorded run output was:
$(cat rec.out)
wanted:
$WANT"
fi

# ── 2: replay with the world REMOVED ────────────────────────────────────────
mv data.txt data.txt.gone
AXON_REPLAY="$J" "$AXON" run p.ax < /dev/null > rep.out 2> rep.err
rep_code=$?
if [ $rep_code -ne 0 ]; then
  bad replay "replay exited $rep_code (expected 0): $(cat rep.err)"
elif [ "$(cat rep.out)" = "$WANT" ]; then
  ok "replay: byte-identical with the file DELETED and stdin CLOSED"
else
  bad replay_values "replay output differs from the recorded run:
$(diff <(printf '%s\n' "$WANT") rep.out || true)"
fi

# ── 3: control — the environment really is gone ──────────────────────────────
#
# Without this, check 2 could pass trivially (a program that never needed the
# world reproduces itself for free).
"$AXON" run p.ax < /dev/null > ctl.out 2>/dev/null
if [ "$(cat ctl.out)" = "$WANT" ]; then
  bad control "an UNREPLAYED run in the stripped environment produced the same \
output — the test is not proving anything (is data.txt still present?)"
else
  ok "control: without the journal the same environment gives different output"
fi

# ── 4: divergence names the first departure ─────────────────────────────────
sed 's|./data.txt|./elsewhere.txt|' p.ax > q.ax
AXON_REPLAY="$J" "$AXON" run q.ax < /dev/null > div.out 2> div.err
div_code=$?
if [ $div_code -ne 11 ]; then
  bad divergence_code "a diverging replay exited $div_code, expected 11"
elif grep -q "event 0" div.err && grep -q "elsewhere.txt" div.err && grep -q "data.txt" div.err; then
  ok "divergence: exit 11, report names event 0 and both paths"
else
  bad divergence_report "the divergence report does not identify the departure point:
$(cat div.err)"
fi

# ── 5: the program cannot SWALLOW a divergence ───────────────────────────────
cat > swallow.ax <<'AXEOF'
fn main() -> i64 {
    match read_file("./wrong.txt") {
        Ok(s) => println("ok")
        Err(e) => println("all good, nothing to see here")
    }
    0
}
AXEOF
AXON_REPLAY="$J" "$AXON" run swallow.ax < /dev/null > sw.out 2>/dev/null
sw_code=$?
if [ $sw_code -eq 11 ] && grep -q "all good" sw.out; then
  ok "unswallowable: program caught the error, printed 'all good', still exit 11"
else
  bad swallow "a program that catches the host error exited $sw_code (expected 11) \
— a guard the audited program can suppress is not a guard"
fi

# ── 6: a SHORT replay is a divergence too ────────────────────────────────────
cat > short.ax <<'AXEOF'
fn main() -> i64 {
    match read_file("./data.txt") { Ok(s) => println("got")  Err(e) => println("no") }
    0
}
AXEOF
AXON_REPLAY="$J" "$AXON" run short.ax < /dev/null >/dev/null 2> short.err
short_code=$?
if [ $short_code -eq 11 ] && grep -q "LESS than the recorded" short.err; then
  ok "short replay: consuming fewer events than recorded is exit 11"
else
  bad short_replay "a replay that stopped early exited $short_code (expected 11): \
$(cat short.err)"
fi

# ── 7: replay performs NO writes ─────────────────────────────────────────────
cat > w.ax <<'AXEOF'
fn main() -> i64 {
    match write_file("./written.txt", "payload") { Ok(u) => println("wrote")  Err(e) => println("failed") }
    0
}
AXEOF
AXON_RECORD="$WORK/wj.txt" "$AXON" run w.ax >/dev/null 2>&1
if [ ! -f written.txt ]; then
  bad write_setup "the recording run did not create written.txt, so check 7 proves nothing"
else
  mv written.txt written.txt.gone
  AXON_REPLAY="$WORK/wj.txt" "$AXON" run w.ax > w.out 2>/dev/null
  w_code=$?
  if [ -f written.txt ]; then
    bad replay_wrote "replaying a run RE-PERFORMED its write — studying an agent \
run must not re-execute its effects"
  elif [ $w_code -eq 0 ] && grep -q "wrote" w.out; then
    ok "replay: the write's recorded outcome is served, the file is NOT written"
  else
    bad replay_write "replaying a write exited $w_code with output: $(cat w.out)"
  fi
fi

# ── 8: recording and replaying at once is refused ───────────────────────────
AXON_RECORD="$WORK/x" AXON_REPLAY="$J" "$AXON" run p.ax </dev/null >/dev/null 2> both.err
both_code=$?
if [ $both_code -eq 2 ] && grep -q "both set" both.err; then
  ok "mutual exclusion: RECORD + REPLAY together is refused (exit 2)"
else
  bad both_set "RECORD+REPLAY together exited $both_code: $(cat both.err)"
fi

# ── 9: off by default ────────────────────────────────────────────────────────
#
# The feature must be invisible when unused. A journal appearing without
# AXON_RECORD, or a changed exit code, would be far worse than the gap it closes.
mv data.txt.gone data.txt
printf 'live-input\n' | "$AXON" run p.ax > off.out 2>/dev/null
off_code=$?
off_want="$(printf 'file: the recorded contents\nstdin: live-input\n')"
if [ $off_code -eq 0 ] && [ "$(cat off.out)" = "$off_want" ]; then
  ok "off by default: unset variables leave behaviour unchanged"
else
  bad off_default "an unconfigured run exited $off_code with:
$(cat off.out)"
fi

# ── 10: a corrupt journal is refused, not partially replayed ────────────────
printf 'this is not a journal line\n' > bad.txt
AXON_REPLAY="$WORK/bad.txt" "$AXON" run p.ax </dev/null >/dev/null 2> corrupt.err
corrupt_code=$?
if [ $corrupt_code -ne 0 ] && grep -q "malformed" corrupt.err; then
  ok "corrupt journal: refused up front, not replayed as a shorter run"
else
  bad corrupt "a corrupt journal exited $corrupt_code: $(cat corrupt.err)"
fi

# ── 11: recording cannot LAUNDER a capability ────────────────────────────────
#
# The recorder wraps the host, so it sits BELOW the capability gate: the sandbox
# check runs at builtin dispatch, before the host is called. If that layering
# were ever inverted, a recording run would perform (and journal) effects the
# sandbox denies — turning an audit feature into a capability bypass, which is
# the worst possible direction for this code to fail in.
cat > cap.ax <<'AXEOF'
fn snoop(x: i64) -> i64 {
    match read_file("/etc/hostname") { Ok(s) => 1  Err(e) => 0 }
}
fn main() -> i64 {
    let p = principal_root("limited", false, false, false, 100)
    let sb = sandbox_create(p, "Pure")
    println("returned {to_str(sandbox_run(sb, \"snoop\", 0))}")
    0
}
AXEOF
AXON_RECORD="$WORK/cj.txt" "$AXON" run cap.ax </dev/null >/dev/null 2> cap.err
cap_code=$?
# The hex of "/etc/hostname" — its presence would mean the read actually happened.
if [ $cap_code -ne 8 ]; then
  bad cap_launder "a sandbox-denied read under AXON_RECORD exited $cap_code, expected 8 \
(sandbox violation). If this is a compile error the check proved NOTHING: $(cat cap.err)"
elif grep -q "2f6574632f686f73746e616d65" "$WORK/cj.txt" 2>/dev/null; then
  bad cap_launder "the sandbox-denied read was PERFORMED and journaled — recording is \
bypassing the capability gate"
else
  ok "capability gate: a denied effect is refused (exit 8) and never journaled"
fi

# ── 13: the transcript is readable AND redacts by default ───────────────────
#
# The human half of replay. A journal is hex, so without this a run is
# reproducible by a machine and opaque to the person who has to sign off on it —
# and they end up trusting the agent's own account of itself, which is what an
# audit exists to avoid.
#
# Redaction is checked as a SECURITY property, not a formatting preference: a
# journal holds the value of every env var the run read, so a transcript that
# leaked credentials by default is one nobody could safely paste into a review.
cat > secretive.ax <<'AXEOF'
fn main() -> i64 {
    let t = env_var("GATE_FAKE_TOKEN")
    match write_file("./touched.txt", "x") { Ok(u) => 0  Err(e) => 0 }
    0
}
AXEOF
GATE_FAKE_TOKEN=sk-gate-canary-9f3b AXON_RECORD="$WORK/tj.txt" "$AXON" run secretive.ax >/dev/null 2>&1
T_DEF="$("$AXON" replay "$WORK/tj.txt" 2>&1)"
T_SHOW="$("$AXON" replay "$WORK/tj.txt" --show-values 2>&1)"
if echo "$T_DEF" | grep -q "sk-gate-canary-9f3b"; then
  bad transcript_leak "the DEFAULT transcript printed a secret the run read — a transcript \
that leaks credentials by default is one nobody can safely share, which defeats its purpose"
elif ! echo "$T_DEF" | grep -q "read env GATE_FAKE_TOKEN"; then
  bad transcript_content "the transcript does not say what happened:
$T_DEF"
elif ! echo "$T_DEF" | grep -q "changed the world : ./touched.txt"; then
  bad transcript_summary "the summary must name what the run changed:
$T_DEF"
elif ! echo "$T_SHOW" | grep -q "sk-gate-canary-9f3b"; then
  bad transcript_show_values "--show-values did not show the value, so the opt-in is broken \
(and this check's redaction result would be meaningless)"
else
  ok "transcript: readable, names what changed, redacts secrets unless --show-values"
fi

# ── 14: diff names the first divergence, and exits 11 ────────────────────────
#
# "What changed between these two runs?" is the question a reviewer actually
# asks. The digests must differ UNDER REDACTION, or reviewing a content change
# would require un-redacting (and pasting secrets) first.
printf 'first' > payload.txt
cat > reader.ax <<'AXEOF'
fn main() -> i64 {
    match read_file("./payload.txt") { Ok(s) => println(s)  Err(e) => println("no") }
    0
}
AXEOF
AXON_RECORD="$WORK/d1.txt" "$AXON" run reader.ax >/dev/null 2>&1
printf 'SECOND' > payload.txt
AXON_RECORD="$WORK/d2.txt" "$AXON" run reader.ax >/dev/null 2>&1

D_SAME="$("$AXON" replay "$WORK/d1.txt" --diff "$WORK/d1.txt" 2>&1)"; d_same_code=$?
D_DIFF="$("$AXON" replay "$WORK/d1.txt" --diff "$WORK/d2.txt" 2>&1)"; d_diff_code=$?
if [ $d_same_code -ne 0 ] || ! echo "$D_SAME" | grep -q "IDENTICAL"; then
  bad diff_identical "a journal diffed against itself must report IDENTICAL and exit 0 \
(got $d_same_code): $D_SAME"
elif [ $d_diff_code -ne 11 ]; then
  bad diff_code "two differing journals must exit 11 (got $d_diff_code): $D_DIFF"
elif ! echo "$D_DIFF" | grep -q "DIVERGE at event 0"; then
  bad diff_point "the diff must name the first divergence point:
$D_DIFF"
elif ! echo "$D_DIFF" | grep -q "DIFFERENT RESULT DATA"; then
  bad diff_axis "the diff must name WHAT differs, not just print two lines:
$D_DIFF"
else
  ok "diff: IDENTICAL vs first-divergence named, exit 11 on difference"
fi

# ── 15: `axon goal` records and replays too ─────────────────────────────────
#
# This is the path where reproducibility matters MOST and where it was missing:
# an optimizer's entire output is score deltas, and a score delta is meaningless
# if the environment moved underneath it. "Variant 7 scored 96" could previously
# only be believed, never re-checked.
#
# The CONTROL half is what makes it non-vacuous: with the input deleted and no
# journal, the optimizer must report a DIFFERENT best score — otherwise the
# replayed run could be reproducing itself for free.
#
# Separate XDG_CACHE_HOME per run on purpose: the provenance log is NOT journaled
# (it is the recorder), so a shared cache would let the recording run's scores
# leak into the replay and make this pass for the wrong reason.
GOALFIX="$REPO/examples/goals/file-budget-goal.md"
if [ ! -f "$GOALFIX" ]; then
  bad goal_fixture "missing $GOALFIX — check 15 cannot run"
else
  printf '250\n' > budget.txt
  XDG_CACHE_HOME="$WORK/gc1" AXON_RECORD="$WORK/goal.journal" \
    "$AXON" goal "$GOALFIX" > grec.out 2>&1
  mv budget.txt budget.txt.gone
  XDG_CACHE_HOME="$WORK/gc2" AXON_REPLAY="$WORK/goal.journal" \
    "$AXON" goal "$GOALFIX" > grep.out 2>&1
  greplay_code=$?
  XDG_CACHE_HOME="$WORK/gc3" "$AXON" goal "$GOALFIX" > gctl.out 2>&1

  rec_best="$(grep -o 'best score: [0-9-]*' grec.out | head -1)"
  rep_best="$(grep -o 'best score: [0-9-]*' grep.out | head -1)"
  ctl_best="$(grep -o 'best score: [0-9-]*' gctl.out | head -1)"
  if [ ! -s "$WORK/goal.journal" ]; then
    bad goal_journal "a goal run under AXON_RECORD wrote no journal — either the wiring is \
absent, or the fixture's scorer stopped touching the host (in which case this proves nothing)"
  elif [ -z "$rec_best" ]; then
    bad goal_record "the recording goal run produced no best score: $(tail -3 grec.out)"
  elif [ "$rep_best" != "$rec_best" ]; then
    bad goal_replay "replayed goal reported '$rep_best' but the recorded run reported \
'$rec_best' (exit $greplay_code) — an optimizer's result is not reproducible: $(tail -3 grep.out)"
  elif [ "$ctl_best" = "$rec_best" ]; then
    bad goal_control "an UNREPLAYED goal run with the input DELETED reported the same \
'$ctl_best' — this check is not proving anything (does the scorer really read the file?)"
  else
    ok "axon goal: optimizer result reproduced from the journal ($rep_best) while an \
unjournaled run in the same stripped environment gives $ctl_best"
  fi
fi

echo "replay_host_gate: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
# A run that checked NOTHING also has zero failures and must not read as a pass —
# the vacuous-pass shape this repo has hit repeatedly.
if [ "$pass" -eq 0 ]; then
  echo "replay_host_gate: SKIP — nothing ran"
  exit 0
fi
echo "replay_host_gate: PASS"
