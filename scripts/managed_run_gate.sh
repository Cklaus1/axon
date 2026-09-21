#!/usr/bin/env bash
# Does a managed run actually OWN its job — result, and cancellation scope?
#
# The failure class this exists for is specific. Gate runs were launched with
# `nohup ... &`; four separate times a notification reported the LAUNCHER's
# exit 0 as the gate's result while the gate had failed. Separately, nine
# `axon test` processes and dozens of CPU spinners leaked for 15+ hours,
# because nothing owned them and their own cleanup could not work.
#
# So this asserts the two properties that were missing, and asserts them
# against a job that has a LIVE GRANDCHILD — the case a naive `kill $pid`
# gets wrong, leaving exactly the orphans that were found.
#
# The CONTROL is the load-bearing half. Cancellation used to mean "kill every
# process whose command line contains this string", which is how you kill
# someone else's work. An unrelated process of the SAME SHAPE must survive.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
RM=scripts/run_managed.sh
fail() { echo "managed_run_gate: FAIL — $*" >&2; exit 1; }

# Distinct, improbable durations so each process is identifiable without
# pattern-matching a command line that would also match this script. A prior
# probe used `pgrep -fc`, which counted ITSELF and reported a false positive.
# UNIQUE PER INVOCATION. Fixed constants made this test non-isolated: a run
# that deliberately failed to clean up (a mutation check, or a crash) left a
# `sleep 7331` behind, and the NEXT run counted that orphan as its own live
# grandchild and reported "survived cancellation" on correct code. A test
# whose verdict depends on what earlier runs leaked is not measuring this run.
# Derived from the pid so concurrent invocations cannot collide either.
VICTIM_SLEEP=$(( 7000 + ($$ % 400) ))
CONTROL_SLEEP=$(( VICTIM_SLEEP + 500 ))
count_sleep() { ps -eo comm,args | awk -v d="$1" '$1=="sleep" && $3==d' | wc -l; }

# An unrelated bystander, owned by nobody, of the same shape as the victim.
setsid sleep "$CONTROL_SLEEP" >/dev/null 2>&1 &
CONTROL_PID=$!
cleanup() { kill -9 "$CONTROL_PID" 2>/dev/null || true; }
trap cleanup EXIT

# ── 1. a job's OWN exit code is recorded, not its launcher's ────────────────
D=$("$RM" start gate_selftest_exit -- bash -c 'exit 7') || fail "start failed"
for _ in $(seq 1 50); do [ "$(cat "$D/status")" != running ] && break; sleep 0.1; done
[ "$(cat "$D/status")" = "exited:7" ] \
  || fail "expected exited:7, got '$(cat "$D/status")' — a launcher's success must never stand in for the job's result"

# ── 1b. a live run reports `running`, not `unknown` ────────────────────────
# A false `unknown` costs as much as a false success. Measured: a healthy gate
# run — supervisor up, cgroup populated, log growing — reported
# "unknown (supervisor gone, scope empty)" because liveness was derived only
# from cgroup population, which is briefly empty while the supervisor is still
# starting. A status that cries unknown on healthy runs is one people learn to
# ignore, and then a REAL unrecorded completion slips past.
D1B=$("$RM" start gate_selftest_live -- bash -c 'sleep 30') || fail "start failed"
ST=$("$RM" status "$D1B")
[ "$ST" = "running" ] \
  || fail "a job that is demonstrably alive reported '$ST' — a false unknown trains the reader to ignore the field"
"$RM" cancel "$D1B" >/dev/null || fail "cancel failed"
rm -rf "$D1B"

# ── 2. cancellation reaches a GRANDCHILD ────────────────────────────────────
# The job forks a child that outlives its parent shell. Killing only the pid
# the supervisor spawned would leave this running.
D2=$("$RM" start gate_selftest_cancel -- bash -c "setsid sleep $VICTIM_SLEEP >/dev/null 2>&1 & sleep 600") \
  || fail "start failed"
for _ in $(seq 1 100); do [ "$(count_sleep $VICTIM_SLEEP)" -ge 1 ] && break; sleep 0.1; done
[ "$(count_sleep $VICTIM_SLEEP)" -ge 1 ] \
  || fail "the fixture never produced a live grandchild — the test would pass without testing anything"

"$RM" cancel "$D2" >/dev/null || fail "cancel reported incomplete"

[ "$(count_sleep $VICTIM_SLEEP)" -eq 0 ] \
  || fail "grandchild survived cancellation — this is the leak that ran for 15 hours"
[ "$(cat "$D2/status")" = "cancelled" ] \
  || fail "status is '$(cat "$D2/status")', not cancelled — a cancelled run must not read as a clean exit"

# ── 2b. the supervisor outlives the shell that launched it ─────────────────
# A real gate run died as `unknown (supervisor gone)` with 17 stages done and
# zero failures: the supervisor was an ordinary background subshell, so when
# the launching shell tore down it took SIGHUP mid-wait and never recorded the
# completion. The earlier version of THIS FILE could not catch that, because it
# only ever launched from a shell that stayed alive — the failure needs the
# launcher to die, so the test has to kill it.
LAUNCHER_OUT=$(mktemp)
setsid bash -c "cd '$PWD' && $RM start gate_selftest_orphan -- bash -c 'sleep 4; exit 5' > '$LAUNCHER_OUT'" &
LAUNCHER=$!
for _ in $(seq 1 100); do [ -s "$LAUNCHER_OUT" ] && break; sleep 0.1; done
D3=$(cat "$LAUNCHER_OUT"); rm -f "$LAUNCHER_OUT"
[ -n "$D3" ] || fail "launcher produced no run dir"
# Kill the launcher's whole session while the job is still running.
kill -9 -- -"$LAUNCHER" 2>/dev/null || kill -9 "$LAUNCHER" 2>/dev/null || true
for _ in $(seq 1 150); do [ "$(cat "$D3/status")" != running ] && break; sleep 0.1; done
[ "$(cat "$D3/status")" = "exited:5" ] \
  || fail "launcher died and the result was lost: status '$(cat "$D3/status")' — a supervisor that dies with its launcher cannot own a long job"
rm -rf "$D3"

# ── 3. the bystander is untouched ───────────────────────────────────────────
[ "$(count_sleep $CONTROL_SLEEP)" -eq 1 ] \
  || fail "cancellation killed an UNRELATED process of the same shape — scope, not substring"

# ── 4. evidence is retained and attributable ────────────────────────────────
# The log must EXIST; it need not be non-empty. A job that prints nothing has
# an empty log, and demanding content here failed a correct run — the check
# was wrong, not the wrapper. The metadata files must have content, because an
# empty `status` or `snapshot` is precisely the unattributable result this
# whole exercise is about.
[ -f "$D2/log" ] || fail "run dir has no log file"
for f in status cmd snapshot scope started_at; do
  [ -s "$D2/$f" ] || fail "run dir is missing or has an empty '$f' — a result you cannot attribute to a source tree is not a result"
done
grep -q '^head=' "$D2/snapshot" || fail "snapshot records no commit"

rm -rf "$D" "$D2"
echo "managed_run_gate: PASS — job result owned, grandchild cancelled, bystander survived, evidence retained"
