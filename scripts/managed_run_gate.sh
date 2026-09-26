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
RUNS=.axon-runs
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

# ── 3b. READY removes startup inference ────────────────────────────────────
# `start` must return only once the run is INTERPRETABLE, so the first status
# read is never a guess. Two misleading `unknown` reports came from inferring
# readiness from an observable side effect (an empty cgroup, which legitimately
# means both "starting" and "gone"). The second time, the cause turned out to
# be that the wait had been deleted outright by an earlier edit — an inference
# that is not even performed is the weakest kind.
D3B=$("$RM" start gate_selftest_ready -- bash -c 'sleep 20') || fail "start failed"
[ -f "$D3B/ready" ] || fail "start returned before the READY record existed"
ST=$("$RM" status "$D3B")
[ "$ST" = "running" ] || fail "first status read after start was '$ST', not running"

"$RM" cancel "$D3B" >/dev/null 2>&1 || true
rm -rf "$D3B"

# PID REUSE, with a premise that actually isolates it.
#
# The first version of this check wrote a bogus start time onto a run whose
# cgroup was genuinely populated — so `running` was the CORRECT answer and the
# guard was never consulted. The failing scenario is the opposite: a STALE run
# directory whose scope is long gone, but whose recorded pid has since been
# recycled by an unrelated process. Without binding the pid to its /proc start
# time, that stale directory reports `running` off a stranger's life.
STALE="$RUNS/gate_selftest_pidreuse"
mkdir -p "$STALE"
setsid sleep "$CONTROL_SLEEP" >/dev/null 2>&1 &
INNOCENT=$!
echo running > "$STALE/status"
echo "pgid:2"  > "$STALE/scope"          # pid 2 is kthreadd: not our process group
echo "$INNOCENT" > "$STALE/supervisor_pid"
echo 1 > "$STALE/supervisor_start"        # deliberately NOT this process's start time
: > "$STALE/ready"
ST=$("$RM" status "$STALE")
case "$ST" in
  lost*) ;;
  *) fail "a recycled pid was read as a live run ('$ST') — liveness must bind the pid to its /proc start time" ;;
esac
# CONTROL: with the correct start time the same directory must read as running,
# or the check would be passing for the wrong reason (e.g. always reporting lost).
awk '{print $22}' "/proc/$INNOCENT/stat" > "$STALE/supervisor_start"
[ "$("$RM" status "$STALE")" = "running" ] \
  || fail "with a MATCHING start time the run must read as running; the pid-reuse check is rejecting everything"
kill -9 "$INNOCENT" 2>/dev/null || true
rm -rf "$STALE"

# ── 3c. terminal state is monotonic ────────────────────────────────────────
# Once a terminal result is durably recorded, `status` must never go back to
# `running` — not even while a descendant is briefly still alive. A lingering
# grandchild does not un-finish a job.
D3C=$("$RM" start gate_selftest_monotonic -- bash -c "setsid sleep $VICTIM_SLEEP >/dev/null 2>&1 & exit 3") \
  || fail "start failed"
for _ in $(seq 1 100); do [ "$(cat "$D3C/status")" != running ] && break; sleep 0.1; done
[ "$(cat "$D3C/status")" = "exited:3" ] || fail "expected exited:3, got '$(cat "$D3C/status")'"
[ "$("$RM" status "$D3C")" = "exited:3" ] \
  || fail "a terminal result was overridden by live-process observation — terminal state must be monotonic"
for p in $(ps -eo pid,comm,args | awk -v d="$VICTIM_SLEEP" '$2=="sleep" && $4==d {print $1}'); do kill -9 "$p" 2>/dev/null; done
rm -rf "$D3C"

# ── 3d. a completed run releases its containment scope ─────────────────────
# Only `cancel` used to release the cgroup, so every NORMALLY COMPLETING run
# leaked one — measured at 71 leaked directories, 71 of the 74 cgroups on the
# host, accumulating across sessions and surviving restarts. The first fix
# called rmdir while the supervisor was still a member of the cgroup, which
# silently does nothing; it has to leave first. Both halves are checked here
# because the second failure mode looks exactly like success.
CG_BEFORE=$(ls -d /sys/fs/cgroup/axon_run_* 2>/dev/null | wc -l)
D3D=$("$RM" start gate_selftest_scope -- bash -c 'exit 0') || fail "start failed"
for _ in $(seq 1 100); do [ "$(cat "$D3D/status")" != running ] && break; sleep 0.1; done
[ "$(cat "$D3D/status")" = "exited:0" ] || fail "expected exited:0, got '$(cat "$D3D/status")'"
# The supervisor writes the status FIRST and releases the scope after (so the
# verdict is durable even if cleanup fails). Counting cgroups the moment status
# flips therefore raced the release — measured flaking 0 -> 1 under load. Wait
# for the supervisor to finish; the leak check then measures the final state.
for _ in $(seq 1 100); do [ -s "$D3D/receipt" ] && ! kill -0 "$(cat "$D3D/supervisor_pid" 2>/dev/null)" 2>/dev/null && break; sleep 0.1; done
CG_AFTER=$(ls -d /sys/fs/cgroup/axon_run_* 2>/dev/null | wc -l)
# Only meaningful where cgroups are actually in use; on a host without them the
# scope is a process group and there is nothing to leak.
case "$(cat "$D3D/scope")" in
  cgroup:*)
    [ "$CG_AFTER" -le "$CG_BEFORE" ] \
      || fail "a completed run leaked its cgroup ($CG_BEFORE -> $CG_AFTER) — the scope must be released, not just abandoned" ;;
esac
rm -rf "$D3D"

# ── 3e. the receipt counts CARGO's tests, not every "N passed" in the log ──
# `tests_passed`/`tests_failed` were summed from `[0-9]+ passed` ANYWHERE in the
# log. Measured on a real axon-core run: a nested tool's line
# (`claims_gate: 5 passed, 1 failed`) was added to cargo's totals, so the
# receipt read 1552/3 while cargo reported 1547/2. That run failed closed by
# luck; any tool printing "N passed" adds phantom PASSES the same way — a
# false-green shape. Only `test result:` lines are cargo's tally.
D3E=$("$RM" start gate_selftest_tally -- bash -c '
  echo "     Running tests/x.rs (target/debug/deps/x-0)"
  echo "some_tool: 900 passed, 0 failed"
  echo "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
  echo "     Running tests/y.rs (target/debug/deps/y-0)"
  echo "test result: FAILED. 4 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
  echo "other: 5 failed checks"
  exit 0') || fail "start failed"
# Wait for the RECEIPT, not the status: status flips first, the receipt is
# written after, and reading it in between makes this check fail for the
# wrong reason.
for _ in $(seq 1 100); do [ -s "$D3E/receipt" ] && break; sleep 0.1; done
[ -s "$D3E/receipt" ] || fail "no receipt written for a completed run"
[ "$(sed -n 's/^tests_passed=//p' "$D3E/receipt")" = 7 ] \
  || fail "receipt tests_passed=$(sed -n 's/^tests_passed=//p' "$D3E/receipt"), expected 7 — non-cargo 'N passed' text was counted as passing tests"
# Both directions: cargo's REAL failures must still be counted (2), and a
# non-cargo "5 failed" must not be.
[ "$(sed -n 's/^tests_failed=//p' "$D3E/receipt")" = 2 ] \
  || fail "receipt tests_failed=$(sed -n 's/^tests_failed=//p' "$D3E/receipt"), expected 2 — cargo failures must count and non-cargo text must not"
rm -rf "$D3E"

# ── 3f. supervisor-owned limits: a memory ceiling and a deadline that FIRE ──
# The limits are passed to `start`, so a contained gate is still a plain
# `gate.sh` command (a `timeout …` prefix made a green strict gate uncitable).
# Only meaningful where cgroups exist; elsewhere `start` must REFUSE a ceiling.
waitrc() { for _ in $(seq 1 300); do [ -s "$1/receipt" ] && return 0; sleep 0.1; done; return 1; }
if [ "$(cat "$D/scope" 2>/dev/null | cut -d: -f1)" = cgroup ] || [ -f /sys/fs/cgroup/cgroup.controllers ]; then
  # (a) a 64M ceiling OOM-kills an allocator; the cgroup's own record makes it
  #     uncitable EVEN THOUGH a wrapper turns the kill into exit 0.
  D3F=$("$RM" start gate_selftest_oom --mem-max 64M --swap-max 0 -- \
        bash -c 'python3 -c "b=bytearray(); [b.extend(bytes(1<<20)) for _ in range(2048)]" & wait; exit 0') \
    || fail "start with --mem-max failed"
  waitrc "$D3F" || fail "no receipt for the OOM self-test"
  grep -q '^mem_max=67108864$' "$D3F/receipt" || fail "receipt does not record the memory ceiling"
  [ "$(sed -n 's/^oom_kills=//p' "$D3F/receipt")" != 0 ] \
    || fail "a 64M ceiling did not OOM-kill a 2 GiB allocator (oom_kills=0) — the ceiling is not applied"
  # Assert the REASON, not just a refusal: this self-test run is uncitable for
  # other reasons too (live tree, no suites), so a bare "verify fails" check
  # passed with the OOM clause deleted (measured).
  # Capture first: under pipefail, `verify | grep -q` fails whenever verify
  # refuses — which it always does here — so the grep's answer was discarded.
  V3F="$("$RM" verify "$D3F" 2>&1)"
  printf '%s' "$V3F" | grep -q 'memory ceiling OOM-killed' \
    || fail "verify did not refuse on the OOM evidence (a wrapper made the job exit 0)"
  rm -rf "$D3F"
  # (b) a 2s deadline stops a long job; the supervisor survives to record it.
  D3G=$("$RM" start gate_selftest_deadline --deadline 2 -- bash -c "sleep $VICTIM_SLEEP") \
    || fail "start with --deadline failed"
  waitrc "$D3G" || fail "no receipt: the deadline killed the supervisor, not just the job"
  grep -q '^deadline_hit=yes$' "$D3G/receipt" || fail "deadline did not fire (receipt: $(grep deadline "$D3G/receipt"))"
  [ "$(count_sleep "$VICTIM_SLEEP")" -eq 0 ] || fail "the deadline left the job's process alive"
  [ "$(count_sleep "$CONTROL_SLEEP")" -ge 1 ] || fail "the deadline killed an unrelated bystander"
  V3G="$("$RM" verify "$D3G" 2>&1)"
  printf '%s' "$V3G" | grep -q 'wall-clock deadline fired' \
    || fail "verify did not refuse on the deadline evidence"
  rm -rf "$D3G"
  # (c) limits do not change what the job IS: gate detection still reads the
  #     command's first token, so a limited gate.sh run stays a gate run.
  D3H=$("$RM" start gate_selftest_limited_gate --mem-max 64M --deadline 60 -- ./scripts/gate.sh --strict --help) \
    || fail "start failed"
  waitrc "$D3H" || fail "no receipt"
  grep -q '^gate_run=yes$' "$D3H/receipt" || fail "a limited gate.sh run was not recorded as a gate run"
  "$RM" cancel "$D3H" >/dev/null 2>&1; rm -rf "$D3H"
fi

# ── 3g. a job's temp is on disk under its run dir, not the RAM /tmp ─────────
D3T=$(env -u TMPDIR "$RM" start gate_selftest_tmp -- bash -c 'echo "$TMPDIR" > "$TMPDIR/where"; cat "$TMPDIR/where"') \
  || fail "start failed"
waitrc "$D3T" || fail "no receipt for the TMPDIR self-test"
case "$(cat "$D3T/log")" in
  "$D3T/tmp") ;;
  *) fail "job TMPDIR was '$(cat "$D3T/log")', expected $D3T/tmp — temp would land in the RAM /tmp" ;;
esac
[ ! -e "$D3T/tmp" ] || fail "the run's temp dir outlived the run"
rm -rf "$D3T"

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
