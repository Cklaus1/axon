# Execution-supervision incident, 2026-09-20/21

Three gate runs, three different outcomes, and the difference between them is
the point: before this, all three would have been indistinguishable.

| run | snapshot | outcome | what it means |
|---|---|---|---|
| gate8 | `7aceea5` | **LOST / unknown** | 17 stages done, 0 failures, then the supervisor died and the completion was never recorded. NOT a pass. |
| gate9 | `b1f6ff8` | **CANCELLED** | deliberately stopped through the wrapper because its source revision had been superseded. NOT a failure. |
| gate10 | `07219e8` | *(authoritative — see result below)* | first run with a clean tree, a known revision, an owned supervisor, persistent evidence, reliable `running`, a cancellable scope, and an unsaturated machine. |

## What actually went wrong

Two operational failures cost more time than any compiler bug, and neither was
the compiler's fault.

**Results were inferred, not owned.** Gate runs were launched with
`nohup ... &`. Four separate times a notification reported the LAUNCHER's exit
0 as the gate's verdict while the gate had failed. Completion was then decided
by matching `gate.sh --strict` against the whole process table — which matched
the MONITORING SHELL's own command line, so a check reported "still running"
with no gate alive. Logs went to `/tmp` and were deleted mid-run by an
unrelated cleanup; the run kept writing to an unlinked inode.

One correction worth preserving: **a deleted pathname does not make an open log
unrecoverable.** On Linux the file persists while any descriptor references it,
and `/proc/<pid>/fd` exposes it. gate6's log was discarded on the reasoning
"pathname gone, therefore lost", which was too strong. It may have been
recoverable at that moment.

**Nothing owned cleanup.** Nine `axon test` processes and dozens of orphaned CPU
spinners ran for 15+ hours. The spinners' own cleanup (`kill %1 %2 ...`) can
never work: job control is disabled in a non-interactive shell, so every batch
leaked permanently. Their parent was dead; `/init` had adopted them.

On the load claim — `ps` %CPU is cumulative CPU time divided by process
lifetime, NOT instantaneous utilisation. What the evidence supports is that
substantial unintended work was removed and load fell sharply afterwards. It
does not support a specific "cores recovered right now" figure, and no speedup
is claimed for the gate; that requires measuring clean runs.

## What the primitive now distinguishes

`running` · `cancelled` · `exited:<code>` · `unknown (completion never recorded)`

Both directions have been demonstrated on real workloads, which is why this is
an execution primitive rather than a shell convenience:

* a real gate whose supervisor died → **unknown**, not pass
* a healthy real gate during startup → **running**, not a false unknown

Four bugs were found in the wrapper itself, each by the acceptance test or by a
real run rather than by review:

1. `D=$(run_managed.sh start ...)` hung for the job's full duration — command
   substitution reads to EOF and the background supervisor inherited the pipe.
2. `echo $$` inside a `( )` subshell writes the PARENT's pid, so the launcher
   was placed in the cgroup and the job was never in scope at all.
3. `setsid` was applied to the child but not the supervisor, so the supervisor
   took SIGHUP when its launching shell tore down — this is gate8.
4. Liveness derived only from cgroup population reported a healthy run as
   unknown, because the cgroup is empty while the supervisor is starting.

And one in the test: fixed sleep durations made it non-isolated, so a mutation
run that deliberately leaked a child caused the NEXT run to report "grandchild
survived" on correct code.

## Deferred — required before this becomes a general facility

Agreed as the next increment, explicitly NOT done now:

* **Atomic READY record.** The supervisor writes READY after establishing its
  identity, scope and child configuration, and `start` returns only then. This
  removes startup inference entirely rather than papering over a transiently
  empty cgroup.
* **An explicit `starting` state.** The transient window gets its own name
  instead of being read as `running` or as `unknown`.
* **PID-reuse protection.** Bind liveness to more than an integer: pid +
  `/proc` start time + run_id + expected command identity. A sufficiently old
  run directory could otherwise observe an unrelated process that inherited the
  pid and report `running`.
* **Monotonic terminal state.** `starting → running → exited:N | cancelled |
  timed_out | lost`. Once a terminal result is durably recorded, `status` must
  never return `running` again, even if a descendant lingers. And process
  disappearance is never, by itself, successful completion.
* **Atomic result writes** (temp file + rename) so no reader observes half a
  result.
* **A full receipt** carrying run_id, source revision, dirty state, command,
  working directory, started/finished timestamps, terminal status and log
  identity — so "gate10 passed" reconstructs as "this exact command passed
  against this exact source state".

## gate10 — the authoritative result

```
run_id           gate10-20260921T044458Z-73309
source_revision  07219e8048161b19e94f65cf5bc38fd453404ec8
dirty_state      no
command          bash scripts/gate.sh --strict
working_dir      /home/cklaus/projects/axon
started_at       2026-09-21T04:44:58Z
finished_at      2026-09-21T05:09:47Z
terminal_status  exited:0
gate_verdict     ✅ gate PASSED
stages           21
test_failures    0
parity           52 harnesses asserted, 2 skipped (Android NDK absent; browser parity opt-in)
log              .axon-runs/gate10-20260921T044458Z-73309/log
```

Two INDEPENDENT signals agree: the supervisor's recorded `exited:0`,
written by a process that actually waited on the child, and the gate's
own `✅ gate PASSED` line. Neither is a launcher's exit status and
neither is a process-table match.

Duration 24m49s. This is a clean BASELINE, not a speedup measurement:
the earlier runs differ in machine load, in stage order, and in outcome,
so no before/after comparison is claimed.

What remains deliberately open, and must not be closed by relabelling:
**36 engine/control states are unknown or silently-ignored** in
`AXON-COMPLETENESS.json`. An unassessed state needs investigation; a
demonstrated ignored safety control needs enforcement or refusal. They
are not interchangeable and neither disappears inside a green gate.

---

## Correction: the "WSL VM loss" premise is not supported

The investigation of the three lost gate runs proceeded on the premise that the
WSL VM disappeared or restarted, and that premise came from me. It does not
survive checking the timestamps.

| run | died (EDT) | nearest observed boot (EDT) | gap |
|---|---|---|---|
| gate8 | 00:29:19 | — | — |
| gate13 | 02:17:55 | 10:42 | **+8h24m** |
| gate14 | 11:49:32 | 13:11 | **+1h22m** |

The reboots happened long AFTER the processes died. They cannot have caused the
losses. Windows-side evidence agrees and adds nothing:

* no WSL, Lxss or WslService event logs exist on this host at all;
* no Hyper-V VM lifecycle events, no unexpected-shutdown entries, no service
  crashes in the System log in any of the three windows;
* the only event near a loss was Service Control Manager 7040 — the Background
  Intelligent Transfer Service changing start type, routine maintenance;
* `Hyper-V-VmSwitch-Operational` events CONTINUE PAST every loss time, including
  a steady ~80-second cadence spanning gate14's 11:49:32. A destroyed VM does
  not keep servicing its switch;
* `dmesg` is readable and contains NO OOM kills.

Run durations rule out a timeout. The three deaths took 24m09s, 22m35s and
21m12s; successful runs took 22m30s, 23m23s and 24m49s. The distributions
overlap completely.

What the three deaths DO share is that each coincided with this session being
interrupted. That makes harness-side teardown of descendant processes the
leading hypothesis — `setsid` places the supervisor in its own session, which
defeats SIGHUP but not a killer that enumerates descendants or kills a cgroup.

A workload-free probe is running to separate the two: a managed job that only
sleeps and logs for 60 minutes. If it dies across an interruption, the gate
workload is exonerated entirely and the managed runner's survival model is the
thing to fix. If it survives while gates die, the workload returns as a
suspect.

No cause is claimed. What is established is narrower and worth stating plainly:
**the VM did not disappear, nothing ran out of memory, and the reboots were
separate later events.** Three of my own explanations have now been refuted by
evidence — stage-17 intrinsic load, cargo feature thrashing, and the VM-loss
premise itself.
