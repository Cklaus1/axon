#!/usr/bin/env bash
# High-resolution sampler for the stage-17 investigation.
#
# Three strict-gate runs (gate8, gate13, gate14) died at
# `per-requirement acceptance gates (R22, R44)` with the WSL VM restarting.
# gate15 passed the same stage. The mechanism is UNKNOWN and this script exists
# to localize it, not to confirm a theory — nothing here should be read as
# evidence for OOM, CPU overload, cgroup failure or cargo recursion unless the
# samples say so.
#
# Samples land OUTSIDE the run directory, because a VM restart is exactly the
# event that makes in-band evidence unavailable, and the run directory is
# managed by the thing that dies.
#
#   stage17_probe.sh start <label>   -> prints the sample file path
#   stage17_probe.sh stop            -> stops the sampler
#   stage17_probe.sh mark <text>     -> append a labelled event line
set -uo pipefail

OUT_DIR="${STAGE17_DIR:-$HOME/axon-scratch/stage17}"
mkdir -p "$OUT_DIR"
PIDFILE="$OUT_DIR/.sampler.pid"
CURRENT="$OUT_DIR/.current"

sample_once() {
  local f="$1"
  {
    printf 'ts=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)"
    printf 'loadavg=%s\n' "$(cat /proc/loadavg 2>/dev/null)"
    # PSI: the direct measure of whether tasks are STALLED on a resource, which
    # loadavg cannot distinguish from tasks merely being busy. Absent on kernels
    # without CONFIG_PSI — recorded as unavailable rather than skipped, so a
    # missing metric is not mistaken for a zero one.
    for r in cpu io memory; do
      if [ -r "/proc/pressure/$r" ]; then
        printf 'pressure_%s=%s\n' "$r" "$(tr '\n' ';' < "/proc/pressure/$r")"
      else
        printf 'pressure_%s=UNAVAILABLE\n' "$r"
      fi
    done
    printf 'mem_available_kb=%s\n' "$(awk '/^MemAvailable:/{print $2}' /proc/meminfo)"
    printf 'mem_free_kb=%s\n' "$(awk '/^MemFree:/{print $2}' /proc/meminfo)"
    printf 'swap_free_kb=%s\n' "$(awk '/^SwapFree:/{print $2}' /proc/meminfo)"
    printf 'committed_kb=%s\n' "$(awk '/^Committed_AS:/{print $2}' /proc/meminfo)"
    # Process and thread counts: a fork/thread explosion would show here and
    # nowhere else.
    printf 'procs=%s threads=%s\n' \
      "$(awk '/^procs_running/{print $2}' /proc/stat)" \
      "$(awk '/^Threads:/{n+=$2} END{print n+0}' /proc/[0-9]*/status 2>/dev/null)"
    printf 'pids_total=%s\n' "$(ls -d /proc/[0-9]* 2>/dev/null | wc -l)"
    # `pgrep -c` EXITS 1 when nothing matches, so `|| echo 0` fired in addition
    # to pgrep's own "0" and emitted two lines — a malformed sample. Counting
    # with grep -c keeps it one value, always.
    _ps="$(ps -eo comm= 2>/dev/null)"
    printf 'cargo=%s rustc=%s sccache=%s\n' \
      "$(printf '%s\n' "$_ps" | grep -cx cargo)" \
      "$(printf '%s\n' "$_ps" | grep -cx rustc)" \
      "$(printf '%s\n' "$_ps" | grep -cx sccache)"
    printf 'cgroups_axon=%s\n' "$(ls -d /sys/fs/cgroup/axon_run_* 2>/dev/null | wc -l)"
    for cg in /sys/fs/cgroup/axon_run_*; do
      [ -d "$cg" ] || continue
      printf 'cgroup=%s pids_current=%s events=%s\n' \
        "$(basename "$cg")" \
        "$(cat "$cg/pids.current" 2>/dev/null || echo NA)" \
        "$(tr '\n' ';' < "$cg/cgroup.events" 2>/dev/null || echo NA)"
    done
    # Disk AND inodes: a full filesystem and an exhausted inode table look
    # completely different and only one of them shows in `df` alone.
    printf 'disk=%s\n' "$(df -P / | awk 'NR==2{print "avail_kb="$4" use="$5}')"
    printf 'inodes=%s\n' "$(df -Pi / | awk 'NR==2{print "ifree="$4" iuse="$5}')"
    printf -- '---\n'
  } >> "$f"
}

case "${1:-}" in
  start)
    label="${2:-stage17}"
    f="$OUT_DIR/${label}-$(date -u +%Y%m%dT%H%M%SZ).samples"
    : > "$f"
    echo "$f" > "$CURRENT"
    setsid bash -c '
      f="$1"; probe="$2"
      while :; do
        bash "$probe" __sample "$f"
        sleep 1.5
      done' _ "$f" "$0" >/dev/null 2>&1 &
    echo $! > "$PIDFILE"
    echo "$f"
    ;;
  __sample) sample_once "$2" ;;
  mark)
    f="$(cat "$CURRENT" 2>/dev/null || echo)"
    [ -n "$f" ] && printf 'MARK ts=%s %s\n---\n' \
      "$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)" "${2:-}" >> "$f"
    ;;
  stop)
    p="$(cat "$PIDFILE" 2>/dev/null || echo)"
    [ -n "$p" ] && kill -- -"$p" 2>/dev/null || true
    [ -n "$p" ] && kill -9 "$p" 2>/dev/null || true
    rm -f "$PIDFILE"
    ;;
  *) echo "usage: stage17_probe.sh {start <label>|stop|mark <text>}" >&2; exit 2 ;;
esac
