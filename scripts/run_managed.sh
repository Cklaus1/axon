#!/usr/bin/env bash
# Own a long-running job: identity, containment scope, retained log, and a
# final result written by the supervisor rather than inferred.
#
# WHY THIS EXISTS. Gate runs were launched with `nohup ... &`, which produced
# four distinct failures, all of which actually happened:
#
#   1. The LAUNCHER's exit status was reported as the JOB's. `nohup` returns 0
#      immediately, so notifications said "exit code 0" four times for gate
#      runs that had failed. A wrapper exiting is not a result.
#   2. Completion was decided by matching `gate.sh --strict` against the whole
#      process table — which matched the MONITORING SHELL's own command line,
#      so a check reported "still running" with no gate alive.
#   3. Logs went to /tmp and were deleted mid-run by an unrelated cleanup; the
#      run kept writing to an unlinked inode.
#   4. Cancellation had no scope to target. Killing by command-line substring
#      is how you kill someone else's process.
#
# A wrapper, not a scheduler: no queue, no daemon, no policy. One job, owned.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNS="$ROOT/.axon-runs"

die() { echo "run_managed: $*" >&2; exit 2; }

# cgroup v2 gives subtree termination (cgroup.kill) and a truthful
# "any processes left?" signal (cgroup.events: populated). Both are PROBED,
# never assumed: this must degrade on a host without cgroup v2, without write
# permission, or in a container that hides it.
cgroup_usable() {
  [ -f /sys/fs/cgroup/cgroup.controllers ] || return 1
  local probe="/sys/fs/cgroup/.axon_probe_$$"
  mkdir "$probe" 2>/dev/null || return 1
  local ok=1
  [ -f "$probe/cgroup.kill" ] && ok=0
  rmdir "$probe" 2>/dev/null
  return $ok
}

# Is this run still live? A run is live if its SUPERVISOR is alive OR its scope
# still holds processes. Both, because each alone gives a wrong answer at one
# end: the cgroup is briefly empty while the supervisor is still starting up
# (which reported a perfectly healthy gate run as `unknown`), and the
# supervisor is gone once the job has legitimately finished.
#
# A false `unknown` costs as much as a false success. A field that cries
# unknown on healthy runs is one people learn to ignore, and then a real
# unrecorded completion slips past — the failure this wrapper exists to stop.
scope_alive() {
  local sp; sp="$(cat "$1/supervisor_pid" 2>/dev/null || echo)"
  if [ -n "$sp" ] && kill -0 "$sp" 2>/dev/null; then
    # Guard against PID REUSE. The recorded start time must match the process
    # currently holding that pid; otherwise this is a different process that
    # merely inherited the number, and reporting `running` off it would be a
    # stale run directory claiming a stranger's life as its own.
    local want; want="$(cat "$1/supervisor_start" 2>/dev/null || echo)"
    local have; have="$(awk '{print $22}' "/proc/$sp/stat" 2>/dev/null || echo)"
    if [ -z "$want" ] || [ "$want" = "$have" ]; then return 0; fi
  fi
  local scope; scope="$(cat "$1/scope" 2>/dev/null || echo)"
  case "$scope" in
    cgroup:*)
      local cg="${scope#cgroup:}"
      [ -d "$cg" ] || return 1
      grep -q 'populated 1' "$cg/cgroup.events" 2>/dev/null ;;
    pgid:*)
      local p="${scope#pgid:}"
      [ "$p" = "pending" ] && return 1
      kill -0 "-$p" 2>/dev/null ;;
    *) return 1 ;;
  esac
}

cmd_start() {
  local name="$1"; shift
  [ "${1:-}" = "--" ] && shift
  [ $# -gt 0 ] || die "no command given"

  local id="${name}-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local dir="$RUNS/$id"
  mkdir -p "$dir" || die "cannot create run dir $dir"

  # WHICH SOURCE SNAPSHOT. A result is meaningless without the tree it judged.
  # `dirty` is recorded as a fact, not a refusal — a gate run on a dirty tree
  # is legitimate, but its result must not later be read as certifying HEAD.
  {
    echo "head=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
    echo "dirty=$( [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ] && echo yes || echo no )"
  } > "$dir/snapshot"
  printf '%s\n' "$*" > "$dir/cmd"
  date -u +%Y-%m-%dT%H:%M:%SZ > "$dir/started_at"
  echo running > "$dir/status"

  echo "pgid:pending" > "$dir/scope"
  if cgroup_usable; then
    local cg="/sys/fs/cgroup/axon_run_$id"
    mkdir "$cg" 2>/dev/null && echo "cgroup:$cg" > "$dir/scope"
  fi

  # The supervisor is launched with `setsid`, in its OWN SESSION, as a mode of
  # this same script. It is not an ordinary background subshell.
  #
  # Measured why: a real gate run died as `unknown (supervisor gone)` with 17
  # stages done and ZERO failures. The supervisor had been a plain `( ... ) &`
  # child of the launching shell, so when that shell tore down it took SIGHUP
  # mid-`wait` and the completion was never recorded. The wrapper reported the
  # ambiguity honestly — which is right, and exactly what it is for — but the
  # ambiguity was avoidable. A supervisor that dies with whoever started it
  # cannot own a long-running job.
  #
  # A separate MODE rather than an inline subshell: the alternative is
  # `setsid bash -c` with the whole supervisor body quoted inside it, which is
  # unreadable and a quoting bug waiting to happen.
  setsid "$0" __supervise "$dir" -- "$@" >/dev/null 2>&1 &

  # Block on the READY record, not on an inferred side effect. This is the
  # whole point of READY: `start` returns only when the run is interpretable.
  for _ in $(seq 1 400); do
    [ -f "$dir/ready" ] && break
    [ "$(cat "$dir/status")" != running ] && break   # a very short job already finished
    scope_alive "$dir" || { [ -f "$dir/supervisor_pid" ] || sleep 0.05; }
    [ -f "$dir/ready" ] && break
    sleep 0.05
  done

  echo "$dir"
}

cmd_status() {
  local dir="$1"
  [ -d "$dir" ] || die "no such run: $dir"
  local st; st="$(cat "$dir/status" 2>/dev/null || echo unknown)"
  # TERMINAL STATE IS MONOTONIC. Once a terminal result is durably recorded it
  # always wins, even if some descendant is briefly still alive — a lingering
  # grandchild does not un-finish a job. And the converse, learned the hard
  # way: process disappearance is NOT successful completion.
  case "$st" in
    exited:*|cancelled|timed_out|lost) echo "$st"; return ;;
  esac
  # Not terminal. Distinguish STARTING from RUNNING from LOST, rather than
  # folding the first into either of the others.
  if [ ! -f "$dir/ready" ]; then
    if scope_alive "$dir"; then echo "starting"; return; fi
    echo "lost (never became ready — supervisor died during startup)"; return
  fi
  # `running` is a CLAIM. Check the scope before repeating it: a supervisor
  # killed mid-wait leaves `running` forever, and reporting that as a live run
  # is the "unknown completion read as success" failure in the other direction.
  if [ "$st" = "running" ] && ! scope_alive "$dir"; then
    st="lost (supervisor gone, scope empty — completion never recorded)"
  fi
  echo "$st"
}

cmd_cancel() {
  local dir="$1"
  [ -d "$dir" ] || die "no such run: $dir"
  echo cancelled > "$dir/status"
  # Stop the supervisor FIRST. It is outside the job's scope by construction
  # (own session, and it joins the cgroup only to place the child), so a
  # scope kill does not reach it — and a surviving supervisor would observe
  # its child die and overwrite `cancelled` with an exit code.
  local sp; sp="$(cat "$dir/supervisor_pid" 2>/dev/null || echo)"
  [ -n "$sp" ] && kill -9 "$sp" 2>/dev/null || true
  local scope; scope="$(cat "$dir/scope" 2>/dev/null || echo)"
  case "$scope" in
    cgroup:*)
      local cg="${scope#cgroup:}"
      # Atomic subtree termination: every descendant, including re-parented
      # ones. This is exactly what a substring kill cannot do safely.
      echo 1 > "$cg/cgroup.kill" 2>/dev/null || true
      for _ in $(seq 1 50); do scope_alive "$dir" || break; sleep 0.1; done
      rmdir "$cg" 2>/dev/null || true ;;
    pgid:*)
      local p="${scope#pgid:}"
      [ "$p" = "pending" ] && die "scope not yet established"
      kill -TERM "-$p" 2>/dev/null || true
      for _ in $(seq 1 30); do scope_alive "$dir" || break; sleep 0.1; done
      if scope_alive "$dir"; then kill -KILL "-$p" 2>/dev/null || true; fi
      for _ in $(seq 1 30); do scope_alive "$dir" || break; sleep 0.1; done ;;
    *) die "no scope recorded for $dir" ;;
  esac
  date -u +%Y-%m-%dT%H:%M:%SZ > "$dir/finished_at"
  if scope_alive "$dir"; then
    echo "cancel INCOMPLETE: scope still populated" >&2; exit 1
  fi
  echo cancelled
}

# The supervisor half. Internal: `__`-prefixed and absent from the usage line
# because nothing outside this file should call it.
cmd_supervise() {
  local dir="$1"; shift
  [ "${1:-}" = "--" ] && shift
  # First act: record own identity. Written to a temp file and RENAMED, so a
  # reader never observes a half-written record — rename is atomic within a
  # filesystem, a partial `echo >` is not.
  #
  # The identity is more than an integer pid. A pid alone is reusable: a
  # sufficiently old run directory could observe an unrelated process that
  # inherited the number and report `running` forever. Binding the pid to its
  # /proc start time (field 22 of /proc/<pid>/stat, in clock ticks since boot)
  # makes the pair unique for the life of the boot — a recycled pid has a
  # different start time and fails the comparison.
  echo $BASHPID > "$dir/.supervisor_pid.tmp"
  mv -f "$dir/.supervisor_pid.tmp" "$dir/supervisor_pid"
  awk '{print $22}' "/proc/$BASHPID/stat" 2>/dev/null > "$dir/.supervisor_start.tmp" || true
  mv -f "$dir/.supervisor_start.tmp" "$dir/supervisor_start" 2>/dev/null || true
  local scope; scope="$(cat "$dir/scope")"
  if [ "${scope#cgroup:}" != "$scope" ]; then
    # Join the cgroup BEFORE spawning, so the child and every descendant it
    # ever forks is inside it from birth. Joining after leaves a window.
    # $BASHPID, not $$: this process's own pid.
    echo $BASHPID > "${scope#cgroup:}/cgroup.procs" 2>/dev/null || true
  fi
  setsid "$@" > "$dir/log" 2>&1 &
  local child=$!
  echo "$child" > "$dir/pid"
  # A setsid child leads its own process group, so pgid == pid. That is the
  # fallback kill scope when cgroups are unavailable.
  [ "$scope" = "pgid:pending" ] && echo "pgid:$child" > "$dir/scope"
  # READY: identity established, scope final, child launched. Everything a
  # reader needs to interpret this run now exists on disk. `start` blocks on
  # this file rather than inferring readiness from an observable side effect,
  # which is what produced two misleading `unknown` reports: the cgroup is
  # legitimately empty while the supervisor is still starting, so "empty" had
  # to mean both "starting" and "gone".
  : > "$dir/.ready.tmp"; mv -f "$dir/.ready.tmp" "$dir/ready"
  wait "$child"
  local code=$?
  date -u +%Y-%m-%dT%H:%M:%SZ > "$dir/finished_at"
  # Never overwrite an explicit `cancelled`: the canceller's verdict is the
  # true one, and `wait` would otherwise report the signal as a plain exit.
  [ "$(cat "$dir/status")" = "cancelled" ] || echo "exited:$code" > "$dir/status"
}

case "${1:-}" in
  __supervise) shift; cmd_supervise "$@" ;;
  start)  shift; cmd_start "$@" ;;
  status) shift; cmd_status "$@" ;;
  cancel) shift; cmd_cancel "$@" ;;
  scope-alive) shift; scope_alive "$@" && echo alive || echo empty ;;
  *) die "usage: run_managed.sh {start <name> -- <cmd...>|status <dir>|cancel <dir>|scope-alive <dir>}" ;;
esac
