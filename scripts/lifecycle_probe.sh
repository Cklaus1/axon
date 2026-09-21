#!/usr/bin/env bash
# Which detachment mechanism actually OWNS a process's lifecycle?
#
# H1: a coding-session/tool interruption kills managed-run descendants despite
# setsid. Three strict-gate runs died with no OOM, no VM teardown, and reboots
# that came 1-8 hours LATER; each coincided with a session interruption.
#
# The conceptual distinction under test: `setsid` detaches TERMINAL/SESSION
# semantics. It does not establish independent LIFECYCLE OWNERSHIP. It defeats
# an ordinary hangup; it does nothing against something that enumerates
# descendants, recursively kills a cgroup, or owns and tears down the whole
# execution scope.
#
# Four heartbeats, identical trivial workload — no cargo, no tests, no Axon —
# so nothing here can be blamed on the gate's workload:
#
#   A  run_managed.sh          (the production mechanism)
#   B  setsid                  (raw session detachment)
#   C  nohup + setsid          (session detachment + stdin/stdout detached)
#   D  systemd-run --user/-    (lifecycle owned by PID 1, not by this shell)
#
# Everything persists OUTSIDE the run directories, because the event under
# study is precisely the one that destroys in-band state.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
# HOME is NOT set for a systemd unit, and under `set -u` that aborted probe D
# before it ever started — reported by my own detection as "systemd-run failed
# to start the unit", which pointed away from the cause. `journalctl -u` named
# it in one line. Default defensively AND pass it explicitly below.
DIR="${LIFECYCLE_DIR:-${HOME:-/root}/axon-scratch/lifecycle}"
mkdir -p "$DIR"

heartbeat() {
  # $1 = name. Writes a timestamped tick every 2s, forever.
  local f="$DIR/$1.beat"
  : > "$f"
  while :; do
    printf '%s tick\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)" >> "$f"
    sleep 2
  done
}

record_meta() {
  # $1 = name, $2 = pid. Everything needed to tell "still the same process"
  # from "a recycled pid", and to see which scope owns it.
  # Defaults matter under `set -u`: a multi-assignment `local` where ANY
  # right-hand side is unset fails as a whole, leaving every name in the
  # declaration unbound — the error then surfaces on a later line and points
  # away from the real cause.
  local n="${1:-}" p="${2:-}"
  local f="$DIR/$n.meta"
  if [ -z "$p" ] || [ ! -d "/proc/$p" ]; then
    printf 'name=%s\nUNAVAILABLE=no live pid captured (got %s)\n' "$n" "${p:-empty}" > "$f"
    return 0
  fi
  {
    printf 'name=%s\n' "$n"
    printf 'pid=%s\n' "$p"
    printf 'ppid=%s\n' "$(awk '{print $4}' "/proc/$p/stat" 2>/dev/null)"
    printf 'pgid=%s\n' "$(awk '{print $5}' "/proc/$p/stat" 2>/dev/null)"
    printf 'sid=%s\n'  "$(awk '{print $6}' "/proc/$p/stat" 2>/dev/null)"
    printf 'starttime=%s\n' "$(awk '{print $22}' "/proc/$p/stat" 2>/dev/null)"
    printf 'cgroup_full=%s\n' "$(tr '\n' '|' < "/proc/$p/cgroup" 2>/dev/null)"
    local cgp; cgp="$(sed -n 's|^0::||p' "/proc/$p/cgroup" 2>/dev/null)"
    printf 'cgroup_path=%s\n' "$cgp"
    printf 'cgroup_events=%s\n' "$(tr '\n' ';' < "/sys/fs/cgroup${cgp}/cgroup.events" 2>/dev/null)"
    printf 'launched_at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  } > "$f"
}

cmd_launch() {
  rm -f "$DIR"/*.beat "$DIR"/*.meta "$DIR"/*.pid 2>/dev/null
  local self; self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

  # A — the production managed runner.
  local d; d=$(bash scripts/run_managed.sh start lifecycle_A -- bash "$self" __beat A)
  echo "$d" > "$DIR/A.rundir"
  sleep 1
  record_meta A "$(cat "$d/pid" 2>/dev/null)"
  printf 'supervisor_pid=%s\n' "$(cat "$d/supervisor_pid" 2>/dev/null)" >> "$DIR/A.meta"

  # B — raw setsid, nothing else.
  setsid bash "$self" __beat B >/dev/null 2>&1 &
  sleep 1
  local bp; bp=$(pgrep -f "$self __beat B" | head -1)
  echo "$bp" > "$DIR/B.pid"; record_meta B "$bp"

  # C — setsid plus stdin closed and output to a persistent file, i.e. the
  # fullest shell-level detachment available without a service manager.
  setsid nohup bash "$self" __beat C </dev/null >"$DIR/C.out" 2>&1 &
  sleep 1
  local cp; cp=$(pgrep -f "$self __beat C" | head -1)
  echo "$cp" > "$DIR/C.pid"; record_meta C "$cp"

  # D — lifecycle handed to PID 1. The point of comparison: if only this
  # survives, shell-level detachment is the wrong abstraction layer.
  if command -v systemd-run >/dev/null 2>&1; then
    systemctl stop axon-lifecycle-D >/dev/null 2>&1
    systemctl --user stop axon-lifecycle-D >/dev/null 2>&1
    local scope=""
    if systemd-run --unit=axon-lifecycle-D --collect --quiet --setenv=LIFECYCLE_DIR="$DIR" bash "$self" __beat D >/dev/null 2>&1; then
      scope="system"
    elif systemd-run --user --unit=axon-lifecycle-D --collect --quiet --setenv=LIFECYCLE_DIR="$DIR" bash "$self" __beat D >/dev/null 2>&1; then
      scope="user"
    fi
    # Ask systemd for the pid rather than pattern-matching the process table.
    # The first version used `pgrep -f` after a 1s sleep, found nothing, and
    # recorded "systemd-run failed to start the unit" — when systemd-run had in
    # fact succeeded. A detection failure reported as a launch failure would
    # have removed the decisive probe from the matrix.
    local dp=""
    if [ -n "$scope" ]; then
      for _ in $(seq 1 40); do
        if [ "$scope" = "user" ]; then
          dp=$(systemctl --user show -p MainPID --value axon-lifecycle-D 2>/dev/null)
        else
          dp=$(systemctl show -p MainPID --value axon-lifecycle-D 2>/dev/null)
        fi
        [ -n "$dp" ] && [ "$dp" != "0" ] && break
        dp=""; sleep 0.25
      done
    fi
    if [ -n "$dp" ]; then
      echo "$dp" > "$DIR/D.pid"; record_meta D "$dp"
      printf 'systemd_scope=%s\n' "$scope" >> "$DIR/D.meta"
    else
      printf 'name=D\nUNAVAILABLE=systemd-run started no unit (scope=%s)\n' "${scope:-none}" > "$DIR/D.meta"
    fi
  else
    printf 'name=D\nUNAVAILABLE=systemd-run not present\n' > "$DIR/D.meta"
  fi
  echo "launched; metadata in $DIR"
}

cmd_observe() {
  # MUST be run from a shell that did not launch them.
  printf '%-3s %-8s %-8s %-8s %-8s %-9s %-7s %s\n' \
    PROBE PID ALIVE PGID SID STARTTIME TICKS CGROUP
  for n in A B C D E F G H; do
    local m="$DIR/$n.meta"
    [ -f "$m" ] || { printf '%-3s (not launched)\n' "$n"; continue; }
    if grep -q '^UNAVAILABLE=' "$m"; then
      printf '%-3s %s\n' "$n" "$(sed -n 's/^UNAVAILABLE=//p' "$m")"; continue
    fi
    local p want have alive ticks cg
    p=$(sed -n 's/^pid=//p' "$m")
    want=$(sed -n 's/^starttime=//p' "$m")
    have=$(awk '{print $22}' "/proc/$p/stat" 2>/dev/null)
    # Identity, not just liveness: a recycled pid must not read as survival.
    if [ -n "$have" ] && [ "$have" = "$want" ]; then alive=YES; else alive=no; fi
    ticks=$(grep -c tick "$DIR/$n.beat" 2>/dev/null || echo 0)
    cg=$(sed -n 's/^cgroup_path=//p' "$m")
    printf '%-3s %-8s %-8s %-8s %-8s %-9s %-7s %s\n' \
      "$n" "$p" "$alive" \
      "$(sed -n 's/^pgid=//p' "$m")" "$(sed -n 's/^sid=//p' "$m")" \
      "$want" "$ticks" "${cg:0:52}"
  done
}

cmd_snapshot() {
  # The observation to make BEFORE interrupting anything: is the "detached"
  # supervisor merely in a different session/PGID while still sitting inside an
  # execution scope the harness owns?
  for n in A B C D E F G H; do
    local m="$DIR/$n.meta"; [ -f "$m" ] || continue
    grep -q '^UNAVAILABLE=' "$m" && { echo "== $n: $(sed -n 's/^UNAVAILABLE=//p' "$m")"; continue; }
    local p; p=$(sed -n 's/^pid=//p' "$m")
    echo "== $n (pid $p) =="
    sed -n 's/^cgroup_full=/  cgroup: /p' "$m"
    command -v pstree >/dev/null 2>&1 && { echo "  ancestry:"; pstree -aps "$p" 2>/dev/null | sed 's/^/    /' | head -8; }
  done
  echo "== this shell =="
  echo "  pid=$$ sid=$(awk '{print $6}' /proc/$$/stat) cgroup: $(tr '\n' '|' < /proc/$$/cgroup)"
}

cmd_stop() {
  for n in A B C D E F G H; do
    local m="$DIR/$n.meta"; [ -f "$m" ] || continue
    local p; p=$(sed -n 's/^pid=//p' "$m"); [ -n "$p" ] && kill -9 "$p" 2>/dev/null
  done
  systemctl stop axon-lifecycle-D 2>/dev/null || systemctl --user stop axon-lifecycle-D 2>/dev/null || true
  local d; d=$(cat "$DIR/A.rundir" 2>/dev/null)
  [ -n "$d" ] && bash scripts/run_managed.sh cancel "$d" >/dev/null 2>&1 || true
  echo stopped
}

# Spawn ONE probe by mechanism, for the 2x2 interruption experiment. A/B/C/D
# were created by tool invocations that completed NORMALLY; E/F/G/H are created
# inside the invocation that will be interrupted. If the harness tracks
# ownership per tool invocation rather than per session, the old set survives
# while the new set dies — a distinction invisible to a single-cohort test.
cmd_spawn() {
  local n="${1:-}" mech="${2:-}"
  local self; self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
  local p=""
  case "$mech" in
    managed)
      local d; d=$(bash scripts/run_managed.sh start "lifecycle_$n" -- bash "$self" __beat "$n")
      echo "$d" > "$DIR/$n.rundir"; sleep 1; p=$(cat "$d/pid" 2>/dev/null) ;;
    setsid)
      setsid bash "$self" __beat "$n" >/dev/null 2>&1 &
      sleep 1; p=$(pgrep -f "$self __beat $n" | head -1) ;;
    nohup)
      setsid nohup bash "$self" __beat "$n" </dev/null >"$DIR/$n.out" 2>&1 &
      sleep 1; p=$(pgrep -f "$self __beat $n" | head -1) ;;
    systemd)
      systemctl stop "axon-lifecycle-$n" >/dev/null 2>&1
      systemd-run --unit="axon-lifecycle-$n" --collect --quiet         --setenv=LIFECYCLE_DIR="$DIR" bash "$self" __beat "$n" >/dev/null 2>&1
      for _ in $(seq 1 40); do
        p=$(systemctl show -p MainPID --value "axon-lifecycle-$n" 2>/dev/null)
        [ -n "$p" ] && [ "$p" != "0" ] && break; p=""; sleep 0.25
      done ;;
    *) echo "unknown mechanism: $mech" >&2; return 2 ;;
  esac
  record_meta "$n" "$p"
  printf 'mechanism=%s\n' "$mech" >> "$DIR/$n.meta"
  echo "$n ($mech) pid=${p:-NONE}"
}

case "${1:-}" in
  __beat)   heartbeat "$2" ;;
  spawn)    shift; cmd_spawn "$@" ;;
  launch)   cmd_launch ;;
  observe)  cmd_observe ;;
  snapshot) cmd_snapshot ;;
  stop)     cmd_stop ;;
  *) echo "usage: lifecycle_probe.sh {launch|observe|snapshot|stop}" >&2; exit 2 ;;
esac
