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
  local snapshot_ref=""
  if [ "${1:-}" = "--snapshot" ]; then
    shift; snapshot_ref="${1:?--snapshot needs a committish}"; shift
  fi
  [ "${1:-}" = "--" ] && shift
  [ $# -gt 0 ] || die "no command given"

  local id="${name}-$(date -u +%Y%m%dT%H%M%SZ)-$$"
  local dir="$RUNS/$id"
  mkdir -p "$dir" || die "cannot create run dir $dir"

  # WHICH SOURCE SNAPSHOT. A result is meaningless without the tree it judged.
  # `dirty` is recorded as a fact, not a refusal — a gate run on a dirty tree
  # is legitimate, but its result must not later be read as certifying HEAD.
  # WHICH SOURCE, AND CAN IT STILL MOVE. Recording `head=` is not enough on its
  # own: the job reads the WORKING TREE, so an edit made while it runs lands in
  # a build that is then reported against the old sha. MEASURED — a strict gate
  # was launched on a clean tree, source files were edited ~2.5 min in while
  # cargo was still compiling those crates, and the resulting PASS could not be
  # attributed to either the committed state or the edited one. It was
  # discarded rather than cited.
  #
  # `--snapshot <committish>` removes the possibility instead of asking people
  # to remember: the job runs in a DETACHED WORKTREE checked out at one commit,
  # which no edit to the developer tree can reach.
  local work=""
  if [ -n "$snapshot_ref" ]; then
    local sha
    sha="$(git -C "$ROOT" rev-parse --verify "${snapshot_ref}^{commit}" 2>/dev/null)" \
      || die "not a commit: $snapshot_ref"
    work="$dir/src"
    git -C "$ROOT" worktree add --detach "$work" "$sha" >/dev/null 2>&1 \
      || die "could not create worktree at $work for $sha"
    {
      echo "mode=worktree"
      echo "head=$sha"
      echo "dirty=no"
      echo "worktree=$work"
    } > "$dir/snapshot"
  else
    {
      echo "mode=live-tree"
      echo "head=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
      echo "dirty=$( [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ] && echo yes || echo no )"
    } > "$dir/snapshot"
  fi
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
      rmdir "$cg" 2>/dev/null || true
      # RECORD the release. `cancel` removed the cgroup but wrote no cleanup
      # record, so `verify` — which reads that record — reported "the
      # containment scope was not released" for a run whose scope WAS
      # released. The cancelled run was non-citable anyway on its status, so
      # the wrong reason cost nothing here; it would have misled the first
      # time it appeared alone.
      if [ -d "$cg" ]; then echo "released=no" >> "$dir/cleanup"; else echo "released=yes" >> "$dir/cleanup"; fi ;;
    pgid:*)
      local p="${scope#pgid:}"
      [ "$p" = "pending" ] && die "scope not yet established"
      kill -TERM "-$p" 2>/dev/null || true
      for _ in $(seq 1 30); do scope_alive "$dir" || break; sleep 0.1; done
      if scope_alive "$dir"; then kill -KILL "-$p" 2>/dev/null || true; fi
      if scope_alive "$dir"; then echo "released=no" >> "$dir/cleanup"; else echo "released=yes" >> "$dir/cleanup"; fi
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
  # Run INSIDE the snapshot when there is one, so the job reads committed
  # bytes rather than whatever the developer tree happens to hold right now.
  local work=""
  work="$(sed -n 's/^worktree=//p' "$dir/snapshot" 2>/dev/null)"
  if [ -n "$work" ] && [ -d "$work" ]; then
    setsid env -C "$work" "$@" > "$dir/log" 2>&1 &
  else
    setsid "$@" > "$dir/log" 2>&1 &
  fi
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
  write_receipt "$dir" "$code"
  # Release the containment scope. Only `cancel` used to do this, so every
  # NORMALLY COMPLETING run leaked its cgroup — measured at 71 leaked
  # directories, 71 of the 74 cgroups on the host, accumulating across
  # sessions. The run directory on disk is the durable record; the cgroup is
  # runtime scaffolding and has no reason to outlive the job.
  #
  # Ordering matters: the status file is written FIRST. If removal fails the
  # result is already durable, and a leaked cgroup is a cleanup problem rather
  # than a lost verdict.
  case "$scope" in
    cgroup:*)
      # LEAVE the cgroup before removing it. The supervisor placed ITSELF
      # inside it (so the child would be in it from birth), and a cgroup with
      # live processes cannot be removed — the first version of this cleanup
      # called rmdir while still a member and silently did nothing, leaving the
      # leak exactly as it was. Moving back to the root cgroup empties it.
      echo $BASHPID > /sys/fs/cgroup/cgroup.procs 2>/dev/null || true
      reap_scope "$dir" "${scope#cgroup:}"
      rmdir "${scope#cgroup:}" 2>/dev/null || true
      ;;
  esac
  # The snapshot worktree is scaffolding too. Remove it AFTER the status file
  # is durable, for the same reason the cgroup is removed after: a failure to
  # clean up must not be able to cost the verdict.
  local work2
  work2="$(sed -n 's/^worktree=//p' "$dir/snapshot" 2>/dev/null)"
  if [ -n "$work2" ] && [ -d "$work2" ]; then
    if git -C "$ROOT" worktree remove --force "$work2" >/dev/null 2>&1; then
      echo "worktree_removed=yes" >> "$dir/cleanup"
    else
      echo "worktree_removed=no" >> "$dir/cleanup"
    fi
  fi

  # Cleanup is EVIDENCE, not a side effect: `verify` reads this to decide
  # whether the scope was genuinely released, so record it either way.
  if [ -d "${scope#cgroup:}" ]; then
    echo "released=no" >> "$dir/cleanup"
  else
    echo "released=yes" >> "$dir/cleanup"
  fi
}

# Processes that legitimately OUTLIVE the job that spawned them.
#
# A build spawns `sccache` as a system-wide singleton daemon. Cargo forks it
# while inside this job's cgroup, and a daemon that double-forks to detach
# from its parent does NOT thereby leave the cgroup — cgroup membership is
# independent of process ancestry. So it stayed a member after the gate
# finished, `rmdir` failed (a populated cgroup cannot be removed), the failure
# was swallowed by `|| true`, and `scope-alive` then reported `alive` for a job
# whose own status file said `exited:0`. MEASURED on finalgate3: pid 407063,
# `/usr/bin/sccache`, `cgroup.events: populated 1`, hours after the gate exited.
#
# Killing it would be wrong — it is shared infrastructure serving other builds,
# not a straggler of this job. So the rule is EVICT, not kill, and only for
# names on this list; anything else still alive in the scope IS a straggler of
# a job that has ended, and gets killed. Both outcomes are recorded.
SHARED_DAEMONS="sccache"

reap_scope() {
  local dir="$1" cg="$2"
  [ -d "$cg" ] || return 0
  local pid comm
  while read -r pid; do
    [ -n "$pid" ] || continue
    comm="$(cat "/proc/$pid/comm" 2>/dev/null || echo '?')"
    if printf '%s\n' $SHARED_DAEMONS | grep -qx -- "$comm"; then
      # Evict to the root cgroup: it keeps running, this scope stops owning it.
      echo "$pid" > /sys/fs/cgroup/cgroup.procs 2>/dev/null \
        && echo "evicted=$comm:$pid" >> "$dir/cleanup" \
        || echo "evict_failed=$comm:$pid" >> "$dir/cleanup"
    else
      kill -TERM "$pid" 2>/dev/null || true
      echo "killed=$comm:$pid" >> "$dir/cleanup"
    fi
  done < <(cat "$cg/cgroup.procs" 2>/dev/null)
  # Give TERM a moment, then take the subtree down hard if anything remains.
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    grep -q 'populated 1' "$cg/cgroup.events" 2>/dev/null || break
    sleep 0.1
  done
  if grep -q 'populated 1' "$cg/cgroup.events" 2>/dev/null; then
    echo 1 > "$cg/cgroup.kill" 2>/dev/null || true
    echo "force_killed=yes" >> "$dir/cleanup"
  fi
}

# ── receipts ────────────────────────────────────────────────────────────────
#
# A receipt binds a RESULT to the exact thing it judged. Without that binding
# a green result is just a green result, and nothing stops it being read as
# evidence about a later commit.
#
# THE FAILURE THIS ANSWERS, which happened: a long `cargo test` was launched
# with `nohup ... &`, the launching shell exited 0, the child was killed
# partway through its largest suite, and the partial tally (704 of 1542) was
# read as a pass. Two separate defects — a launcher's status standing in for
# the job's, and a partial run counted as a complete one — so the receipt
# records BOTH the child's own exit status and whether every suite that
# started also reported.
write_receipt() {
  local dir="$1" code="$2"
  local head dirty tree_digest log_digest
  head="$(sed -n 's/^head=//p' "$dir/snapshot" 2>/dev/null)"
  dirty="$(sed -n 's/^dirty=//p' "$dir/snapshot" 2>/dev/null)"
  # A dirty tree is recorded as a DIGEST, not just a flag, so two different
  # dirty trees cannot share one receipt.
  tree_digest="$(git -C "$ROOT" status --porcelain 2>/dev/null | sha256sum | cut -d' ' -f1)"
  log_digest="$(sha256sum "$dir/log" 2>/dev/null | cut -d' ' -f1)"

  # SUITES STARTED vs SUITES THAT REPORTED. cargo prints `Running <binary>`
  # when a suite starts and `test result:` when it finishes; a killed or
  # crashed suite produces the first and never the second, which is exactly
  # how a partial run passes for a complete one.
  local started reported passed failed
  started="$(grep -cE '^[[:space:]]+(Running|Doc-tests)' "$dir/log" 2>/dev/null | head -1)"
  reported="$(grep -cE '^test result:' "$dir/log" 2>/dev/null | head -1)"
  passed="$(grep -oE '[0-9]+ passed' "$dir/log" 2>/dev/null | awk '{s+=$1} END{print s+0}')"
  failed="$(grep -oE '[0-9]+ failed' "$dir/log" 2>/dev/null | awk '{s+=$1} END{print s+0}')"
  : "${started:=0}"; : "${reported:=0}"

  {
    echo "schema=axon-run-receipt/1"
    echo "command=$(cat "$dir/cmd" 2>/dev/null)"
    echo "head=${head:-unknown}"
    echo "tree=${dirty:-unknown}"
    echo "tree_digest=$tree_digest"
    echo "source_mode=$(sed -n 's/^mode=//p' "$dir/snapshot" 2>/dev/null)"
    echo "toolchain=$(rustc --version 2>/dev/null | tr ' ' '-')"
    # ENVIRONMENT. Axon's behaviour is steered by AXON_* variables — an
    # effect ceiling, a mock/replay mode, a clock, a seed — so "the tests
    # passed" is a claim about the environment they passed in. Recorded as a
    # sorted name=value digest plus the names themselves: the names make a
    # surprising run legible at a glance, the digest makes two runs
    # comparable without publishing any value.
    echo "env_axon_names=$(env | grep -oE '^AXON_[A-Z0-9_]+' | sort | tr '\n' ',' | sed 's/,$//')"
    echo "env_axon_digest=$(env | grep -E '^AXON_' | sort | sha256sum | cut -d' ' -f1)"
    echo "cargo_profile=${CARGO_PROFILE:-debug}"
    echo "child_exit=$code"
    echo "started_at=$(cat "$dir/started_at" 2>/dev/null)"
    echo "finished_at=$(cat "$dir/finished_at" 2>/dev/null)"
    echo "log_sha256=${log_digest:-unknown}"
    echo "suites_started=$started"
    echo "suites_reported=$reported"
    echo "tests_passed=${passed:-0}"
    echo "tests_failed=${failed:-0}"
  } > "$dir/receipt.tmp"
  mv -f "$dir/receipt.tmp" "$dir/receipt"
}

# ── verify: is this run citable as evidence about a commit? ─────────────────
#
# Four facts have to hold TOGETHER, and each was independently wrong at some
# point this project: the source was an immutable snapshot (a live-tree run was
# edited mid-build), the job actually executed tests (a `grep "^test result"`
# reports nothing on a compile error, and nothing read as green), the exit
# status is the JOB's and not a launcher's (`nohup` returned 0 four times for
# failed runs), and the scope was released (a lingering daemon left
# `scope-alive` contradicting a recorded `exited:0`).
#
# Exit 0 only when all four hold. Anything else prints why and exits 1, so a
# caller can branch on the status rather than on prose.
cmd_verify() {
  local dir="${1:?verify needs a run dir}"
  shift || true
  # `--for <commit>`: the commit this evidence is being cited FOR. Defaults to
  # the current HEAD, which is the case that matters — a receipt must not be
  # readable as evidence about work committed after it ran.
  local want=""
  if [ "${1:-}" = "--for" ]; then
    shift; want="${1:?--for needs a commit}"; shift || true
  fi
  [ -d "$dir" ] || die "no such run: $dir"
  local bad=0
  local mode head st
  mode="$(sed -n 's/^mode=//p' "$dir/snapshot" 2>/dev/null)"
  head="$(sed -n 's/^head=//p' "$dir/snapshot" 2>/dev/null)"
  st="$(cat "$dir/status" 2>/dev/null || echo unknown)"

  if [ "$mode" != "worktree" ]; then
    echo "  NOT CITABLE: source mode is '${mode:-unset}', not 'worktree' — a"
    echo "               live-tree run can be edited while it builds, so its"
    echo "               result is not attributable to any one commit"
    bad=1
  fi
  case "$head" in
    ''|unknown) echo "  NOT CITABLE: no source commit recorded"; bad=1 ;;
  esac
  if [ "$st" != "exited:0" ]; then
    echo "  NOT CITABLE: status is '$st', not 'exited:0'"
    bad=1
  fi

  # EXECUTION, positively. Absence of failure is not evidence of testing.
  # NOT `grep -c ... || echo 0`: on no matches grep prints "0" AND exits 1, so
  # the fallback appends a second "0" and the count becomes the string "0\n0",
  # which is not an integer and silently defeats the comparison below. Caught
  # by mutation-testing this very check with a compile-error log.
  local suites
  suites="$(grep -c '^test result: ok' "$dir/log" 2>/dev/null | head -1)"
  [ -n "$suites" ] || suites=0
  if [ "${suites:-0}" -lt 1 ]; then
    echo "  NOT CITABLE: the log records 0 passing test suites — a build"
    echo "               failure prints no 'test result' line at all, and"
    echo "               empty output is not green"
    bad=1
  fi
  if grep -qE '^  FAIL|test result: FAILED' "$dir/log" 2>/dev/null; then
    echo "  NOT CITABLE: the log contains failures"
    bad=1
  fi

  if ! grep -q '^released=yes' "$dir/cleanup" 2>/dev/null; then
    echo "  NOT CITABLE: the containment scope was not released"
    bad=1
  fi

  # ── receipt ───────────────────────────────────────────────────────────
  if [ ! -f "$dir/receipt" ]; then
    echo "  NOT CITABLE: no receipt — this run predates receipts or died"
    echo "               before recording one"
    bad=1
  else
    local r_head r_exit r_started r_reported r_failed r_log
    r_head="$(sed -n 's/^head=//p' "$dir/receipt")"
    r_exit="$(sed -n 's/^child_exit=//p' "$dir/receipt")"
    r_started="$(sed -n 's/^suites_started=//p' "$dir/receipt")"
    r_reported="$(sed -n 's/^suites_reported=//p' "$dir/receipt")"
    r_failed="$(sed -n 's/^tests_failed=//p' "$dir/receipt")"
    r_log="$(sed -n 's/^log_sha256=//p' "$dir/receipt")"

    # THE CHILD's status, not the launcher's. This is the distinction the
    # whole receipt exists for.
    if [ "${r_exit:-1}" != "0" ]; then
      echo "  NOT CITABLE: the job's own child exited $r_exit"
      bad=1
    fi

    # A PARTIAL RUN IS NOT A PASS. Every suite that started must have
    # reported; a killed suite prints `Running` and never `test result:`.
    if [ "${r_started:-0}" -ne "${r_reported:-0}" ]; then
      echo "  NOT CITABLE: $r_started suite(s) started but only $r_reported"
      echo "               reported — the run did not finish, and its partial"
      echo "               tally must not be read as a complete result"
      bad=1
    fi
    if [ "${r_failed:-0}" -ne 0 ]; then
      echo "  NOT CITABLE: $r_failed test(s) failed"
      bad=1
    fi

    # The log must still be the log this receipt was written for.
    local now_log
    now_log="$(sha256sum "$dir/log" 2>/dev/null | cut -d' ' -f1)"
    if [ -n "$r_log" ] && [ "$r_log" != "unknown" ] && [ "$now_log" != "$r_log" ]; then
      echo "  NOT CITABLE: the log has changed since the receipt was written"
      bad=1
    fi

    # ── STALENESS ────────────────────────────────────────────────────────
    # A receipt for commit X must not certify commit Y. This is the check
    # that makes a receipt evidence rather than a souvenir.
    local target
    target="$(git -C "$ROOT" rev-parse "${want:-HEAD}" 2>/dev/null)"
    if [ -n "$target" ] && [ -n "$r_head" ] && [ "$r_head" != "$target" ]; then
      echo "  STALE: this receipt certifies $r_head"
      echo "         but evidence was requested for $target"
      echo "         (a receipt created before later commits cannot certify them)"
      bad=1
    fi
  fi

  if [ "$bad" -ne 0 ]; then
    echo "run: $dir"
    return 1
  fi
  echo "CITABLE  commit=$head  status=$st  suites=$suites  scope=released"
  echo "         child_exit=0  suites_reported=$(sed -n 's/^suites_reported=//p' "$dir/receipt")  tests_passed=$(sed -n 's/^tests_passed=//p' "$dir/receipt")"
  echo "run: $dir"
  return 0
}

case "${1:-}" in
  __supervise) shift; cmd_supervise "$@" ;;
  start)  shift; cmd_start "$@" ;;
  status) shift; cmd_status "$@" ;;
  cancel) shift; cmd_cancel "$@" ;;
  scope-alive) shift; scope_alive "$@" && echo alive || echo empty ;;
  verify) shift; cmd_verify "$@" ;;
  *) die "usage: run_managed.sh {start <name> [--snapshot <committish>] -- <cmd...>|status <dir>|verify <dir> [--for <commit>]|cancel <dir>|scope-alive <dir>}" ;;
esac
