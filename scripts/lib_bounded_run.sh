#!/usr/bin/env bash
# lib_bounded_run.sh — run a child under an OS-enforced memory ceiling AND a
# wall-clock deadline, and report resource exhaustion as a distinct outcome.
#
# WHY THIS EXISTS (R15 §13 B3 host-safety): the Asyncify driver once grew node's
# RSS at ~0.5 GiB/s until the WSL VM died, taking the whole dev environment with
# it. A `.ax` fixture — or any future runtime bug — must not be able to do that.
# Three independent layers, outermost first:
#
#   1. wall-clock deadline      (`timeout`; always available)
#   2. OS/cgroup memory ceiling (cgroup v2 memory.max + memory.swap.max)
#   3. Node/V8 heap ceiling     (caller's business — SECONDARY defense only)
#
# Layer 3 is NOT sufficient on its own and must never be the only bound:
# `--max-old-space-size` governs V8's old space, while Wasm linear memory and
# Buffer/native allocations live outside it. Measured on this host, a node
# process under `--max-old-space-size=512` reached 24.8 GiB RSS. Layers 1+2 are
# what actually hold; layer 2 is what bounds PEAK memory.
#
# Usage:
#   source scripts/lib_bounded_run.sh
#   bounded_run <mem_bytes_or_human> <timeout_secs> <cmd> [args...]
#
# CHOOSING A CEILING: bound the PROGRAM UNDER TEST, not a compiler. The ceiling
# applies to the whole subtree, so a harness that shells out to `cargo build` puts
# rustc/cc/ld under the same cap — and a 2 GiB ceiling that is generous for an
# interpreted `.ax` program will OOM-kill a codegen build (observed). Either build
# the artifact BEFORE the bounded region and bound only the run (preferred — it is
# what the failure is about), or raise the ceiling to fit the toolchain (6 GiB+).
#
# Exit status (also placed in BOUNDED_RUN_CLASS):
#   OK                  child's own exit status (0)
#   RESOURCE_EXHAUSTED  137 — cgroup OOM-killed the child (or a descendant)
#   TIMEOUT             124 — wall-clock deadline hit
#   FAIL                child's own nonzero status
#
# A child killed by a signal ALWAYS yields nonzero here. Never let a wrapper
# swallow that into 0 — `bash -c '... & wait'` does exactly that (verified: the
# child is OOM-killed and the wrapper still returns 0), which would turn a fired
# safeguard into a passing test.

# Resolve a human size ("1G", "512M", "1073741824") to bytes.
_br_bytes() {
  local v="$1"
  case "$v" in
    *[gG]) echo $(( ${v%[gG]} * 1024 * 1024 * 1024 ));;
    *[mM]) echo $(( ${v%[mM]} * 1024 * 1024 ));;
    *[kK]) echo $(( ${v%[kK]} * 1024 ));;
    *)     echo "$v";;
  esac
}

# True when we can create a cgroup v2 node with a memory controller.
_br_cgroup_ok() {
  [ "$(stat -fc %T /sys/fs/cgroup 2>/dev/null)" = "cgroup2fs" ] || return 1
  [ "$(id -u)" = "0" ] || return 1
  grep -qw memory /sys/fs/cgroup/cgroup.controllers 2>/dev/null || return 1
  # A cgroup created at the ROOT is not subject to the internal-process
  # constraint that makes `echo +memory > <our-cgroup>/cgroup.subtree_control`
  # fail with EIO when processes live directly in our own cgroup (they do, under
  # WSL's /wsl-user/distro-*/non-systemd). `systemd-run --user` is unavailable
  # here for the same reason: no XDG_RUNTIME_DIR and no session bus.
  local probe="/sys/fs/cgroup/.br_probe_$$"
  mkdir -p "$probe" 2>/dev/null || return 1
  local ok=1
  [ -f "$probe/memory.max" ] && ok=0
  rmdir "$probe" 2>/dev/null
  return $ok
}

bounded_run() {
  local mem="$1" secs="$2"; shift 2
  local mem_b; mem_b=$(_br_bytes "$mem")
  local swap_b=$(( mem_b / 4 ))
  BOUNDED_RUN_CLASS=""; BOUNDED_RUN_PEAK=""; BOUNDED_RUN_ALSO_TIMEOUT=""
  local rc

  if _br_cgroup_ok; then
    local cg="/sys/fs/cgroup/axon_bounded_$$_${RANDOM}"
    mkdir -p "$cg" || { echo "bounded_run: cannot create $cg" >&2; return 70; }
    echo "$mem_b"  > "$cg/memory.max"      2>/dev/null
    echo "$swap_b" > "$cg/memory.swap.max" 2>/dev/null
    echo 0         > "$cg/memory.peak"     2>/dev/null || true

    # The child joins the cgroup and execs, so the limit binds the process AND
    # every descendant it forks (a bash wrapper's node child is covered).
    ( echo $BASHPID > "$cg/cgroup.procs" && exec timeout --signal=TERM --kill-after=5s "${secs}s" "$@" )
    rc=$?

    BOUNDED_RUN_PEAK=$(cat "$cg/memory.peak" 2>/dev/null || echo "")
    local oomk; oomk=$(awk '/^oom_kill /{print $2}' "$cg/memory.events" 2>/dev/null || echo 0)

    # Atomic subtree teardown, so nothing survives the boundary as an orphan.
    if [ -f "$cg/cgroup.kill" ]; then echo 1 > "$cg/cgroup.kill" 2>/dev/null || true; fi
    local spin=0
    while [ -s "$cg/cgroup.procs" ] && [ $spin -lt 50 ]; do sleep 0.1; spin=$((spin+1)); done
    rmdir "$cg" 2>/dev/null || true

    # Classify RESOURCE_EXHAUSTED on the cgroup's own OOM EVIDENCE, not only on
    # exit status — because a wrapper can LAUNDER the kill. Measured here: a child
    # OOM-killed inside `bash -c '<cmd> & wait'` gives the wrapper rc=0 while the
    # cgroup records oom_kill=1. Trusting rc alone would report that as OK, i.e. a
    # fired safeguard looking like a passing test. memory.events is the cgroup's
    # account of what it did and no wrapper can rewrite it, so it is the authority.
    #
    # Note a graceful-failure case that is NOT exhaustion: `WebAssembly.Memory.grow`
    # returns an error instead of allocating when the ceiling is reached, so a
    # program that catches it and keeps looping hits the deadline with oom_kill=0
    # (measured: max=1634 enforcements, oom 0, peak pinned at the ceiling). TIMEOUT
    # is the honest classification there — the ceiling held; nothing was killed.
    if [ "${oomk:-0}" != "0" ] || [ "$rc" = "137" ]; then
      BOUNDED_RUN_CLASS="RESOURCE_EXHAUSTED"
      # Preserve whether the deadline ALSO fired, for diagnosis.
      [ "$rc" = "124" ] && BOUNDED_RUN_ALSO_TIMEOUT=1
      return 137
    fi
  else
    # No cgroup available: the deadline still binds, but PEAK MEMORY DOES NOT.
    # Say so loudly rather than let a caller believe it is protected.
    echo "bounded_run: WARNING no cgroup memory ceiling available — wall-clock deadline only" >&2
    BOUNDED_RUN_CLASS_DEGRADED=1
    timeout --signal=TERM --kill-after=5s "${secs}s" "$@"
    rc=$?
  fi

  case "$rc" in
    0)   BOUNDED_RUN_CLASS="OK";;
    124) BOUNDED_RUN_CLASS="TIMEOUT";;
    137) BOUNDED_RUN_CLASS="RESOURCE_EXHAUSTED";;
    *)   BOUNDED_RUN_CLASS="FAIL";;
  esac
  return $rc
}
