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

scope_alive() {
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

  # The supervisor runs in the background but KEEPS THE CHILD'S HANDLE and
  # writes the result itself. The status file is written by something that
  # actually waited, not by a later guess about the process table.
  (
    scope="$(cat "$dir/scope")"
    if [ "${scope#cgroup:}" != "$scope" ]; then
      # Join the cgroup BEFORE spawning, so the child and every descendant it
      # ever forks is inside it from birth. Joining after leaves a window.
      # $BASHPID, NOT $$. Inside a ( ) subshell `$$` is still the PARENT
      # shell's pid, so `echo $$` moved the launching script into the cgroup
      # and left the actual job outside it. The script then exited, the cgroup
      # read as empty, and a live run reported "supervisor gone". Measured
      # before the fix: scope empty immediately after a successful start.
      echo $BASHPID > "${scope#cgroup:}/cgroup.procs" 2>/dev/null || true
    fi
    setsid "$@" > "$dir/log" 2>&1 &
    child=$!
    echo "$child" > "$dir/pid"
    # A setsid child leads its own process group, so pgid == pid. That is the
    # fallback kill scope when cgroups are unavailable.
    [ "$scope" = "pgid:pending" ] && echo "pgid:$child" > "$dir/scope"
    wait "$child"
    code=$?
    date -u +%Y-%m-%dT%H:%M:%SZ > "$dir/finished_at"
    # Never overwrite an explicit `cancelled`: the canceller's verdict is the
    # true one, and `wait` would otherwise report the signal as a plain exit.
    [ "$(cat "$dir/status")" = "cancelled" ] || echo "exited:$code" > "$dir/status"
    # stdout/stderr are closed off the CALLER'S PIPE deliberately. `start` is
    # meant to be used as `D=$(run_managed.sh start ...)`, and command
    # substitution reads until EOF — not until the foreground command exits.
    # A background subshell that inherits the pipe therefore holds it open for
    # the life of the JOB, so the caller blocks for the full run and `start`
    # silently stops being asynchronous. Measured: a 600-second fixture hung
    # its caller for 600 seconds.
  ) >/dev/null 2>&1 &

  # Return only once the scope is REAL. Otherwise `start` hands back a run
  # whose scope is still "pending", and an immediate `status` or `cancel`
  # races the supervisor — `cancel` would refuse, and `status` would call a
  # live run "unknown". The window is milliseconds; the confusion it causes is
  # exactly the "is this thing actually running?" question this wrapper exists
  # to answer.
  for _ in $(seq 1 100); do
    [ "$(cat "$dir/scope")" != "pgid:pending" ] && scope_alive "$dir" && break
    [ "$(cat "$dir/status")" != running ] && break   # a very short job already finished
    sleep 0.05
  done
  echo "$dir"
}

cmd_status() {
  local dir="$1"
  [ -d "$dir" ] || die "no such run: $dir"
  local st; st="$(cat "$dir/status" 2>/dev/null || echo unknown)"
  # `running` is a CLAIM. Check the scope before repeating it: a supervisor
  # killed mid-wait leaves `running` forever, and reporting that as a live run
  # is the "unknown completion read as success" failure in the other direction.
  if [ "$st" = "running" ] && ! scope_alive "$dir"; then
    st="unknown (supervisor gone, scope empty — completion never recorded)"
  fi
  echo "$st"
}

cmd_cancel() {
  local dir="$1"
  [ -d "$dir" ] || die "no such run: $dir"
  echo cancelled > "$dir/status"
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

case "${1:-}" in
  start)  shift; cmd_start "$@" ;;
  status) shift; cmd_status "$@" ;;
  cancel) shift; cmd_cancel "$@" ;;
  scope-alive) shift; scope_alive "$@" && echo alive || echo empty ;;
  *) die "usage: run_managed.sh {start <name> -- <cmd...>|status <dir>|cancel <dir>|scope-alive <dir>}" ;;
esac
