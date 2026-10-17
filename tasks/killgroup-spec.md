# DRAFT — Whole-job cancellation for the remaining supervisors (KG)

**This draft MUST pass a Step 1 adversarial review before any build.** Nothing here is approved.
[REVISED: Step 1 review done 2026-10-17 (ReviewKillgroup) against `main` 43fa1857; human decisions of 2026-10-17
applied under Needs human. Scope after review and decisions: KG1, KG2, KG3, KG5 (added). KG4 excluded (NH2).]

Source: `tasks/opportunities.md:2491-2493` ([P2] "Other supervisors kill only their direct child", deferred by the
gaps run). Base: `main` 71d6f503, worktree `gaps-int`. [REVISED: was 71d6f503 → 43fa1857, reason: main moved one
commit (tasks/ only); `git diff 71d6f503 43fa1857 -- crates scripts` is empty, so every line number below still
holds.] Every site was re-measured against this tree on 2026-10-17;
the listed line numbers had not moved. Reference pattern: `crates/axon-os/src/runtime.rs` `run_bounded_capped`
(G2, CHANGELOG top section) and `crates/axon-intent/src/synth.rs` `kill_group`.
[REVISED: review re-checked every runtime.rs, synth.rs, generate.rs, cli.rs, cortex.rs, gate.sh,
refusal-coverage and release_manifest anchor cited in KG1-KG3 and found each where stated. The four dropped sites'
own line references (runner.rs `:2946-2960`, `:3034-3056`, psv `:344-374`, git_data `:33-66`, firecracker
`:255-261`, main.rs `:6463-6489`) were NOT re-measured by the review: they are dropped by human decision (NH3), so
nothing depends on them.]

Mission: where a supervisor's child can have descendants, a timeout or a teardown kills the whole job, a terminal or
service signal to the supervisor still reaches the job, and no pipe held by a leftover descendant can stall the
supervisor. Where the child cannot have descendants, say so with evidence and change nothing.

Shared acceptance: `scripts/gate.sh` shows no new failure against the 71d6f503 baseline [REVISED: baseline 43fa1857,
same tree under crates/ and scripts/]; `python3 -B
scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay green, every new site in a protected file
carrying a killed mutation row or a reasoned exemption and every moved anchor re-pinned;
`python3 -B scripts/g01_anchors.py` green [REVISED: added, reason: it is the fast-tier check (`gate.sh:334`) that every
active row's `old` text occurs exactly once; KG1 deletes code from runtime.rs, the file of 11 active rows];
`crates/axon-core/tests/process_control_guard.rs` and `crates/axon-core/tests/release_manifest.rs` stay green [REVISED:
release_manifest added, reason: a new workspace member fails `release_manifest_covers_every_workspace_member` until
classified]; `AXON-COMPLETENESS.json` row "cancellation reaches the
whole job" (`:754-771`) gets the new evidence paths and `AXON-COMPLETENESS.md` is regenerated (integrator, last).

## Re-measurement (all six listed sites)

| Site | Spawns | Descendants plausible? | Kill path today | Verdict |
|---|---|---|---|---|
| `axon-cortex/src/generate.rs:552` | the operator's `cmd:PATH` generator (`:483`), any program, typically a script that runs a model CLI | **yes**: measured on this host, SIGKILL to a `#!/bin/sh` script running `sleep 4711` (dash, not exec'd) left the `sleep` running, re-parented | `child.kill()` + `wait()` at the deadline; no forwarding of signals to the supervisor; drains and stdin writer joined unbounded on the normal-exit path (`:564-566`) | **KG2, KG3** |
| `axon-reflex/src/lib.rs:610` | the caller's sidecar exe (`:555-560`), long-lived | yes for a wrapper or a sidecar that forks; the in-tree sidecar (`src/bin/sidecar.rs:16-17`) forks nothing | `Drop` kills the direct child only | **KG4** [REVISED: was KG4 → excluded, reason: NH2 excluded by human; see KG4] |
| `axon-cortex/src/runner.rs:3095` | the registered `axon-test-local` binary as `axon test … --completion-key-stdin` (`:2946-2960`), always | **no**: under a completion key `axon test` removes `Exec` from its effect ceiling before program code runs (`axon-core/src/main.rs:6463-6489`), proven by `axon-core/tests/psv_test_selection.rs:208` `holding_a_completion_key_spawns_nothing`; `axon test` itself spawns nothing (the only `Command::new` in `main.rs` are `:3317` rust-lld and `:9499` axon-vm, neither on the test path) | `child.kill()`; leader also has `PR_SET_PDEATHSIG` (`:3034-3056`) | dropped [DECIDED by human: NH3] |
| `axon-psv/src/runner.rs:410` | pinned `axon test --completion-key-stdin` in the guest | **no**: `Exec` removed twice (`without_exec`, `:344`, and the interpreter as above); only production caller is the in-guest `axon-psv-runner` (`src/bin/axon-psv-runner.rs:89`, built by `scripts/build-guest-image.sh:229`) | `child.kill()`; `PR_SET_PDEATHSIG` + `NO_NEW_PRIVS` (`:345-374`) | dropped [DECIDED by human: NH3] |
| `axon-fabric/src/git_data.rs:396` | `/usr/bin/git cat-file --batch` (`GIT_BIN`, `:29`; `:273-282`) | **no** [INFERENCE from git's documented behaviour]: env cleared, lazy fetch off, `protocol.allow=never`, hooks/fsmonitor off (`:33-66`); `cat-file --batch` without `--filters`/`--textconv` runs no helper | `Drop`: close stdin, `kill`, `wait` | dropped [DECIDED by human: NH3] |
| `axon-vm/src/firecracker.rs:223,448,466` | the Firecracker VMM (`:255-261`), an absolute regular file (`FirecrackerBin::at`, `:783-793`) | **no** in production [INFERENCE: Firecracker is one multi-threaded process; the jailer named at `:361-363` execs it in place]; a forking wrapper is possible but a `sudo` wrapper leaves a root VMM that a user `killpg` cannot reach either | `kill` + `wait` in `LaunchGuard::drop` and at the sentinel grace / deadline | dropped [DECIDED by human: NH3] |

Census (`.kill()` in non-test code): the six above plus `synth.rs:280` (already group-killed) — nothing unlisted.
[REVISED: confirmed by the review's own `\.kill\(\)` grep over `crates/*/src`: the hits beyond these are the
bystander cleanups in `runtime.rs:1721`/`synth.rs:710` (unit tests), runtime.rs's own fallback `:694`, and
`axon-fabric/src/sealed_exec.rs:637`, which is inside a test (`ATTACK:` assertion at `:640`).]

[REVISED: added, reason: a census by `.kill()` misses supervisors that never kill. One more supervisor has
job-group children and no signal forwarding: the `axon-intent` binary (`src/main.rs`, 8 lines, calls `cli::run`).
Its children are already out of its process group — synth.rs `run_bounded` (`:262`, `process_group(0)`) and
`AxonCoreRuntime::imports_digest` → `run_bounded_capped` (`intent cli.rs:328, :452`) — so Ctrl-C or SIGTERM to
`axon-intent` kills it and leaves `axon intent compile --gen` running with no deadline left [INFERENCE from code,
not observed]. The G2d gap, in a binary G2 did not cover. → **KG5**.]

## Design decision: one shared crate, `axon-jobctl` (engineering, decided)

The G2d machinery is process-global: a fixed registry of job groups (`runtime.rs:391`, 64 slots), a termination flag
(`:394`), and SIGINT/SIGTERM/SIGHUP handlers (`:383`, `:399-415`). A second copy in another crate would install a
second handler over the first and each would know only its own registry; `axon-fabric` already links `axon-os`,
`axon-cortex` and `axon-vm`. So the code lives once. A per-site copy is rejected for that reason, and because the
signal-safety details took 19 review findings in G2.

Where it lives: `axon-reflex` has no `libc` (`Cargo.toml` deps: serde_json, axon-cortex, axon-loop-contracts);
`axon-cortex` does not depend on `axon-os` and should not (it would pull `axon-audit` + `axon-attest` into the repair
loop for a process helper). A new leaf crate depending only on `libc` fits all callers.
[REVISED: decision kept, rationale corrected. Alternatives measured (crate deps from each `Cargo.toml`):
(a) a module in `axon-os` with `axon-cortex` → `axon-os`: no cycle (axon-os links sha2, serde, serde_json,
axon-audit, libc, axon-attest), and the weight is small (audit/attest are sha2/serde/hex/base64 crates cortex mostly
has), so "pulls audit + attest" is not the reason. The reasons are: axon-os is the most-edited authority crate in this
run (AR1-AR2 edit runtime.rs, supervisor.rs, approval.rs, cli.rs), and an axon-os → cortex edge would put cortex,
loop-contracts, loop, reflex and cortex-policy-adapter downstream of every one of those edits — more rebuilds per lane
iteration, and every axon-os file change selecting the cortex suites in an impact-based mutation run (the BROAD stop,
`opportunities.md:2475-2477`); it also makes the repair loop depend on the supervisor crate for one helper.
(b) a module in `axon-cortex` (already has libc): axon-os would depend on cortex and through it `axon-ai` (reqwest)
— rejected. (c) an existing libc-free low-dependency crate (`axon-workspace-recipe`, `axon-loop-contracts`):
unrelated purpose, and recipe is in the guest's trust story — rejected. A libc-only leaf changes rarely after KG1, so
its rebuild fan-out is paid once. With KG4 excluded the callers are axon-os, axon-intent and axon-cortex.]
Cost, all paid in this change:
workspace member + `Cargo.lock`; `-p axon-jobctl` in the runtime clippy line (`gate.sh:835-842`, enforced by the
coverage check `:844-862`); a class-A entry in `governance/release-verification.json` (enforced by
`axon-core/tests/release_manifest.rs:56`); it enters the refusal-coverage protected scope automatically, because
`axon-os` and `axon-cortex` are in the measured protected set (`protected_crates()` today: attest, audit, cortex,
fabric, guest-init, guest-kernel, loop, loop-contracts, os, psv, vm, workspace-recipe). The guest build (`--locked`,
static musl) is unaffected: `axon-psv` does not gain the dependency.
[REVISED: precision, reason: verified in code. `protected_crates()` (`v022_refusal_coverage.py:183-190`) is the
closure over `[dependencies]` and `[target.*.dependencies]` path deps from ROOT_CRATES and the guest packages, so the
crate enters scope whether added as a plain or a `cfg(unix)` target dependency. The class-A entry needs a non-empty
`required` (`release_manifest.rs:94`): `cargo test --locked -p axon-jobctl`; `gate.sh:943-959` runs every A/B
`required` command, so no separate gate.sh test line is needed. `lockfile_gate.sh` (`gate.sh:934`) requires the
committed `Cargo.lock` to be current. Guest packages (`build-guest-image.sh:196,212,229`: axon-core, axon-guest-init,
axon-psv) link neither os nor cortex, so the guest binaries are unchanged [INFERENCE: not rebuilt by the review].]

**Signal-handler ownership** [REVISED: added, reason: the draft did not say who installs the handler when several
crates in one binary link the crate.] Canon:
- Cargo builds one copy of a path crate per binary, so the registry, the flag and the handler exist once per process
  however many crates use `Job`. That is what the crate is for.
- Library code never installs a handler. Only a binary's `main` calls `forward_termination_signals(mode)`: `axon-os`
  (`cli.rs:522`, `OnSignal::Continue`), `cortex` (KG3, `OnSignal::Die`), `axon-intent` (KG5, `OnSignal::Die`).
- The mode is one process-global atomic read by the one handler. The first call sets it; a later call with the same
  mode is a no-op; a later call with a different mode panics (a programming error in one binary, caught by that
  binary's own tests). Rationale: a silent last-wins would let one library's choice change another's exit semantics.
- `axon-fabric` links axon-os, axon-cortex and axon-vm but spawns no `Job` (checked: no `AxonCoreRuntime`,
  `CommandGenerator` or `RealSynthesizer` in `crates/axon-fabric|axon-loop|cortex-policy-adapter|axon-reflex/src`),
  so it installs nothing and nothing changes for it. A binary that later spawns a `Job` and wants signal reach must
  call `forward_termination_signals` in its `main`; the crate's doc comment says so.
- `Job::spawn`'s blocked-mask window and `pre_exec` disposition reset are unconditional (as today in axon-os), so
  they are correct whether or not a handler is installed.

## KG1 — Extract the job-group machinery; axon-os and axon-intent use it (clean cutover)

Current: `runtime.rs` owns `TERMINATION_SIGNALS`, `JOB_GROUPS`, `on_termination_signal` (pid > 1 guard `:408`),
`disposition`/`set_disposition`/`termination_mask`/`swap_thread_mask` (`:425-465`), `forward_termination_signals`
(`:475-486`), `termination_signal` (`:490`), `JobGroupSlot` (`:511-528`), `spawn_registered` (`:540-564`),
`job_has_exited` (`:568-589`), `DRAIN_GRACE`/`wait_readable`/`join_drains` (`:598-652`), and the inner `kill_group`
(`:687-696`). `synth.rs` has its own `process_group(0)` (`:262`) and `kill_group(pid)` (`:208-243`).
Change: move these into `crates/axon-jobctl/src/lib.rs` behind a small API — `Job::spawn(&mut Command)` (sets
`process_group(0)`, blocks the termination signals across spawn + registration, installs the existing `pre_exec`
mask/disposition reset), `Job::kill()` (`killpg(pgid, SIGKILL)`, falling back to `Child::kill` on failure),
`Job::has_exited()` (WNOWAIT), `Job::reap()` (frees the slot, then waits), `wait_readable(fd, &stop)`,
`join_within(grace, handles, &stop)`, `forward_termination_signals(OnSignal)`, `termination_signal()`. Capture policy
stays with each caller (`read_bounded` head + tail stays in axon-os). The pid > 1 guard stays in the handler only, where
a free slot (0) is reachable (G2 review #17: a guard on an owned child's pid can never fire). `run_bounded_capped` and
`synth.rs` `run_bounded` call the crate; their private copies are deleted. `OnSignal::Continue` is today's axon-os
behaviour (flag, poll loop writes the record, exit 128+sig); `cli.rs:522`/`:590` are otherwise unchanged.
[REVISED: API gaps closed, reason: the listed API cannot express what the callers do today.
- `Job` must give the caller its pipes (`child_mut() -> &mut Child`, or `take_stdin/stdout/stderr`) and its pid.
- `pub const DRAIN_GRACE` is exported: axon-os's `read_bounded` prints it (`runtime.rs:338`) and stays in axon-os.
- `Drop` of an un-reaped `Job` kills the group, frees the slot, then reaps (same order as `reap`). Reason: an early
  return (e.g. generate.rs's `try_wait` error arm `:544`, which today returns without killing) must not leak a
  registered, running job.
- `OnSignal::Die` is defined and unit-tested here, not in KG3: one jobctl change instead of two (dev-loop: every
  jobctl edit rebuilds os, intent, cortex and everything downstream of them; see Gates).
- Stdio stays set in `run_bounded_capped` (`:720-722`), and `job_child_command` is untouched: the VALUE_EXEMPT
  ordinals `run_bounded_capped` 1-3 (`v022_refusal_coverage.py:4135-4137`) and the active g01 rows on runtime.rs
  (M1793-M1795, M1797, M2315-M2317, M2319, M3561, M3575: admission scan, child env, imports digest — none in the
  moved code) keep matching exactly once.
- `cli.rs:522`/`:590` change their path to `axon_jobctl::…`; no re-export from `axon_os::runtime` (clean cutover;
  the only callers are those two lines, checked by grep).
- synth.rs keeps its sequential post-exit reads (non-goal); only spawn/kill/reap move. Its procps history comment
  (`:209-231`) moves with the kill into the crate's `Job::kill` doc, since that is where a future `Command::new("kill")`
  would be written.]
Acceptance: behaviour-preserving, so the oracle is the existing G2 suite — `axon-os/tests/supervisor_signals.rs`,
the G2a/G2c/G2d tests in `axon-os/tests/legacy_adapter.rs`, and `synth.rs`'s bystander test
(`:689` `the_timeout_kill_reaches_only_the_childs_own_group`) — green
unchanged. [REVISED: confirmed valid as an oracle: no unit test in runtime.rs references the moved private items
(grep of `runtime.rs:900-1897` for JOB_GROUPS, spawn_registered, job_has_exited, wait_readable, join_drains,
JobGroupSlot, kill_group, DRAIN_GRACE, termination_signal: none), so the suite compiles unchanged. synth.rs's two
timeout tests (`:689`, `:722`) test the bystander and the timeout, not the grandchild; the crate unit test below is
what pins the grandchild for synth's path.] Crate unit tests: a grandchild of a killed `Job` is gone (`/proc/<pid>/cmdline` oracle, pid file written by
the job, grandchild stdio to `/dev/null`), a bystander in the test's own group survives, `reap` frees the slot before
the wait. [REVISED: plus `OnSignal::Die` in a re-executed copy of the test binary (`current_exe()` with a selector
env var): after SIGTERM to it, it dies by SIGTERM (`ExitStatusExt::signal`) and its registered job's grandchild is
gone; and a second `forward_termination_signals` with the other mode panics.] Refusal coverage: the axon-os exemptions at `v022_refusal_coverage.py:3438-3439` and `:3452-3468` move to the
crate's path; their reasons are re-derived for the crate's real caller set (axon-os, axon-intent, cortex generate,
reflex), not copied.
[REVISED: precise, reason: the gate refuses an exemption whose anchor matches nothing (`:6779`), so every entry is
handled in the same commit, each by kind:
- moves to `crates/axon-jobctl/src/lib.rs` (text identical after the move, or re-pinned): `:3438` killpg, `:3439`
  `.process_group(0)`, `:3452` handler killpg, `:3453-3458` disposition/sigaction/pthread_sigmask, `:3459`
  `termination_signal`, `:3460` `pre_exec`, `:3461-3462` the `Interrupted` arm, `:3468` `wait_readable`.
- stays in runtime.rs but its text changes: `:3463-3464` (`kill_group(&mut child)` becomes the `Job::kill` call) —
  re-pinned.
- stays in runtime.rs unchanged, reason text false after KG3/KG5: `:3465-3467` `Verdict::Halted` — `_G2D` says the
  only callers of `forward_termination_signals|termination_signal|spawn_registered` are run_bounded_capped and
  cmd_run; after this change cortex and axon-intent call it too and `spawn_registered` no longer exists. `_G2D`/`_G2C`
  are rewritten.
- New reason for the crate's entries: callers are axon-os's legacy adapter (`_ACR`, off route), axon-intent (not a
  protected crate) and cortex `CommandGenerator` (`_CTX`), checkable by `grep -rn axon_jobctl crates/*/src`; Fabric
  spawns no Job. Reflex is not a caller (KG4 excluded).
- Run the gate once after the move: any additional crate site it reports is one runtime.rs already had under the
  same form; it gets the moved reason, never a new "REMAINDER".]

## KG2 — The cortex generator is killed as a job, and never stalls the loop

Current (`generate.rs:483-566`): spawned in cortex's own group; at the deadline only the direct child is killed; on a
normal exit `out_t.join()`, `err_t.join()` and `writer.join()` wait for EOF/completion with no bound, so a generator
that leaves `sleep 600 &` behind (stdout inherited) stalls `cortex repair` until the leftover exits, and the deadline
no longer applies (the loop has already broken). The existing deadline test `cli.rs:1587` (`hang.sh`: `sleep 600`)
already leaves an orphaned `sleep 600` per run [INFERENCE: same mechanism as the measurement above; not observed on a
test run].
Change: spawn via `Job::spawn`; the deadline calls `Job::kill` then `reap`; drains poll with `wait_readable` and are
joined with `join_within(DRAIN_GRACE)` on every path; the stdin writer is joined within the same grace and otherwise
detached (it only holds a pipe; after a group kill its write fails with EPIPE). Leftovers after a normal exit are not
killed (NH1). Output caps (`MAX_GENERATOR_READ`) and the `Declined`/`Unavailable` mapping are unchanged.
[REVISED: added constraints, reason: anchors and an unstated behaviour change.
- Keep the three `Stdio::piped()` calls in `propose` in their order: VALUE_EXEMPT `propose` 1-3
  (`v022_refusal_coverage.py:4022-4024`) address them by ordinal; re-pin if the builder moves.
- The detached writer can block forever: a leftover holds the stdin pipe's read end too, so with a prompt larger than
  the pipe buffer `write_all` never returns. Acceptable in the `cortex` binary (exits after the episode); the doc
  comment says a long-lived host leaks one blocked thread per such generator.
- Behaviour change, stated: the generator leaves the terminal's foreground group. A generator that reads `/dev/tty`
  (e.g. an interactive login prompt) now gets SIGTTIN and stops until the deadline, where today it could prompt
  [INFERENCE: standard job-control semantics, not tested here]. Its stdin is already a pipe, so no non-interactive
  generator is affected; README/CORTEX.md say so.]
Acceptance (fail-first, all through the production `cortex repair --generator cmd:` path in `tests/cli.rs`):
(a) `hang.sh` writes `$$` and a background `sleep 4720`'s pid, then `wait`s; after the 700 ms deadline the run ends
code 24 and the grandchild is gone within 5 s (`/proc` + cmdline); today it survives. (b) a generator prints a body,
starts `sleep 60 &` without redirecting stdout, exits 0; `cortex repair` returns within 20 s (today ≥ 60 s); the test
kills the leftover itself with `libc::kill`. Mutations: `process_group(0)` removed in the crate → (a) red; `killpg`
replaced by `Child::kill` → (a) red; `join_within` replaced by plain `join` → (b) red.
[REVISED: confirmed fail-first: (a) today `child.kill()` signals the dash leader only, and the grandchild is in
cortex's group, untouched; (b) today `out_t.join()` (`:564`) waits for the leftover's EOF. With `process_group(0)`
mutated away, `killpg(pid)` on a non-leader fails ESRCH and the fallback `Child::kill` leaves the grandchild, so the
first mutation is killed by (a) as claimed. (b)'s wait budget must also cover the stderr drain: the leftover holds
stderr as well.]

## KG3 — `cortex` forwards termination signals to its jobs

Current: `bin/cortex.rs` (`main` at `:326`) installs no handler. Today terminal Ctrl-C reaches the generator because it
shares the foreground group; KG2 removes that, and SIGTERM to cortex (a CI `timeout`, a service manager) never
reached it. Change: `main` calls `forward_termination_signals(OnSignal::Die)` first. `Die` (new in the crate): the
handler SIGKILLs every registered group (as G2d), then sets the signal's disposition to `SIG_DFL` and `raise`s it, so
cortex dies by the same signal as today (no record to write, nothing to resume; a `Continue` flag would be ignored
outside a poll loop and swallow Ctrl-C). Already-ignored signals stay ignored (`nohup` keeps working).
[REVISED: `Die` is added to the crate in KG1 (dev-loop, one jobctl change); KG3 is the cortex call and its test.
Mechanics confirmed sound: `sigaction`, `killpg` and `raise` are async-signal-safe; the handler runs with the
termination signals blocked (`sa_mask`, `set_disposition` `:439`), so the raised signal stays pending until the
handler returns and is then delivered under `SIG_DFL`. The axon test children cortex spawns in its own group
(runner.rs) are unchanged: Ctrl-C still reaches them directly and `PR_SET_PDEATHSIG` covers SIGTERM to cortex.]
Acceptance (fail-first): spawn `cortex repair` in its own process group with a `hang.sh` generator; once both pid
files exist, `libc::kill(cortex_pid, SIGTERM)`; cortex exits by SIGTERM (`ExitStatusExt::signal`) and the generator
and its grandchild are gone within 5 s; today both survive. Uses SIGTERM, not SIGINT: a background job of a
non-interactive shell (`gate.sh`'s `par`) starts with SIGINT ignored, which would make a SIGINT test vacuous.
Mutations: the `main` call deleted → red; `Die`'s re-raise removed → red (cortex keeps running).
[REVISED: confirmed fail-first (today SIGTERM to cortex's pid reaches only cortex). The re-raise mutation is red on
the exit-status assertion: without it the handler returns, the poll loop sees the killed generator and cortex exits
normally with a code, not a signal.]

## KG4 — The reflex sidecar is torn down as a job

[REVISED: was in scope → EXCLUDED, reason: NH2 excluded by human. KG4 cannot be done without the Ctrl-C behaviour
change: a group kill needs the sidecar to lead its own process group, which is exactly what removes it from the
terminal's foreground group. The alternatives that keep it in the caller's group do not reach its tree reliably:
walking `/proc` children is racy against pid reuse and misses re-parented descendants; `PR_SET_CHILD_SUBREAPER` is
process-wide and a non-goal; cgroups need privileges. Nothing below is built; the text stays for the record.
Remaining killgroup scope: KG1, KG2, KG3, KG5.]
Current: `SidecarBackend::spawn` (`lib.rs:554-577`) leaves the sidecar in the caller's group; `Drop` (`:608-613`) kills
the direct child, so a wrapper's real worker survives unless it exits on stdin EOF. Only caller in the tree:
`tests/principal_isolation.rs:156` (public API). Change: spawn via `Job::spawn`, `Drop` → `Job::kill` + `reap`. Doc
comment on `SidecarBackend` states that terminal Ctrl-C no longer reaches the sidecar group unless the host binary calls
`forward_termination_signals` (NH2). Acceptance (fail-first): a wrapper sidecar script records `$$` and a background
`sleep 4721` pid, then `exec cat >/dev/null`; dropping the backend leaves neither alive within 5 s; a bystander in the
test's group survives. Mutation: `Drop` back to `Child::kill` → red. Refusal coverage: none — `axon-reflex` is not in
the protected set (it is only a `[dev-dependencies]` entry of `axon-loop`, `Cargo.toml:21,30`; measured via
`protected_crates()`).

## KG5 — `axon-intent` forwards termination signals to its jobs [REVISED: added, P3, reason: see Re-measurement]

Current: `crates/axon-intent/src/main.rs:6-8` installs no handler; its synth and imports-digest children run in their
own groups (synth.rs `:262`; `run_bounded_capped` via `AxonCoreRuntime::imports_digest`), so SIGINT/SIGTERM to
`axon-intent` orphans them. Change: `main` calls `axon_jobctl::forward_termination_signals(OnSignal::Die)` first
(after KG1 both child paths are registered `Job`s). No other change; not a behaviour change for a run that is not
signalled. Acceptance (fail-first): `axon-intent compile` with `AXON_BIN` pointing at a script that writes `$$` and a
background `sleep 4722` pid then `wait`s, `AXON_INTENT_TIMEOUT_MS` large, spawned in its own process group; once
both pid files exist, `libc::kill(intent_pid, SIGTERM)`; `axon-intent` exits by SIGTERM and both pids are gone
within 5 s; today both survive. [INFERENCE: `compile` reaches `RealSynthesizer` without `AXON_AI_MOCK`
(`intent cli.rs:68-72`); the builder confirms the exact verb and args before writing the test.] Mutation: the `main`
call deleted → red (axon-intent is not a protected crate, so this row is for G01 completeness, not refusal coverage).
Sequenced after KG1; independent of KG2/KG3.

## Constraints every item inherits

- **No `kill` binary.** `process_control_guard.rs:88-94` fails on `Command::new("kill"|"/bin/kill"|"/usr/bin/kill"|
  "pkill"|"killall")` anywhere under `crates/`, tests included. Tests signal and clean up with `libc::kill`/`killpg`
  on typed pids; liveness via `/proc/<pid>/cmdline` (pid reuse) as in `axon-os/tests/supervisor_signals.rs:52-57`.
  Shell scripts may use the builtin (`process_control_guard.rs:22-25`). Never `killpg(0)`; tests that signal a
  supervisor spawn it with `process_group(0)` (`supervisor_signals.rs:59-84`).
- **Refusal coverage** (`PRIV_FORM`, `v022_refusal_coverage.py:4688-4703`): every new `.process_group(`, `libc::killpg(`,
  `libc::sigaction(`, `libc::pthread_sigmask(`, `.pre_exec(` in a protected file (the crate; `generate.rs` and
  `bin/cortex.rs` under the `_CTX` reason at `:1554`) gets a g01 row killed by the tests above or a checkable exemption.
  [REVISED: expected new sites outside the crate: none — generate.rs and cortex.rs call `Job`/`forward_…`, which match
  no form. `libc::raise(` (Die) is not in the form list (`signal|sigaction|sigprocmask|pthread_sigmask|kill|killpg`,
  g01 M2128's anchor); its `SIG_DFL` reset goes through `set_disposition`'s `sigaction`, an existing moved site.]
- **g01 rows** (format as `v022_g01_mutations.py:7844-7847`): about six, listed under each item; package `axon-os`
  rows belong to the serial shard.
  [REVISED: counted: KG1 0 new rows (behaviour-preserving; its sites carry moved exemptions), KG2 3, KG3 2, KG5 1 = 6.
  All mutate `crates/axon-jobctl/src/lib.rs`, `generate.rs`, `bin/cortex.rs` or intent `main.rs`; their test
  packages are axon-cortex (`--test cli`) and axon-intent, not axon-os, so none is a serial-shard row unless the
  builder adds one with an axon-os test.]
- **Anchors kept unique** [REVISED: added]: `g01_anchors.py` counts each active row's `old` text in its file; no
  text KG1 leaves in runtime.rs may duplicate an existing anchor (e.g. a second `cmd.env_clear();` block).

## Gates and cost budgets

- Lane loop: `cargo test --locked -p axon-jobctl -p axon-os -p axon-intent -p axon-reflex` and
  `cargo test --locked -p axon-cortex --test cli` (needs the interpreter built: `gate.sh:678-681`). Budget ≤ 15 min
  wall [INFERENCE]; CPU-bound, competes with any concurrent lane build.
  [REVISED: dev-loop, per gate (Balanced mode, human decision: lanes run `gate.sh --fast` + crate tests; the
  integrator runs the full gate once per wave). Measured figures cited; the rest `[INFERENCE]`.
  | Gate | Cost per run (resource) | Re-run forced by | Skip when | Queues behind / ahead |
  |---|---|---|---|---|
  | `cargo test --locked -p axon-jobctl` | seconds; cold build <1 min (libc only) [INFERENCE] (CPU) | any jobctl edit | jobctl unchanged | nothing |
  | `cargo test --locked -p axon-os -p axon-intent` | ~5-10 min warm (same estimate as the approval spec's `-p axon-os`) [INFERENCE] (CPU) | jobctl, runtime.rs, synth.rs, intent main.rs | none of those changed | competes with the AR1 work in the same lane: run once per item, not per edit |
  | `cargo test --locked -p axon-cortex --test cli` | interpreter build (cold axon-core `--no-default-features` bin, minutes) + cli suite [INFERENCE] (CPU) | jobctl, generate.rs, cortex.rs, cli.rs | those unchanged | the fast tier runs the whole cortex suite anyway (`gate.sh:381`); run the filtered tests during the edit loop, the fast tier once per item |
  | `scripts/gate.sh --fast` | axon-core `--no-default-features` tests (~10 min, `gate.sh:940` comment) + cortex + psv suites + vm_parity + clippy of changed crates; ~15-25 min [INFERENCE] (CPU, whole host) | once per item, before handing the lane to the integrator | — | do not run with 5 lanes building (`opportunities.md:2500-2501`) |
  | `v022_refusal_coverage.py`, `g01_anchors.py` | seconds, no cargo (CPU) | any edit to a protected file or to the registries | — | run after every KG1 move step: they are the cheap way to catch a dangling exemption |
  | `v022_g01_mutations.py --only=<6 rows>` | each cell rebuilds the mutated crate's dependents in the test package's closure + one test; ~2-5 min per cell [INFERENCE] → ≤ 30 min serial (CPU) | a row's file or test changes | row file hashes unchanged | after the item's tests are green; never in parallel with another lane's mutation run |
  | full `scripts/gate.sh` | 24-40 min measured (`tasks/r51-baseline.md:21-40`; one 52 min run cited by the approval spec), whole host | integrator, once per wave | — | alone on the host |
  Biggest foreseeable sink: every jobctl edit invalidates axon-os, axon-intent, axon-cortex and their dependents
  (loop-contracts, loop, fabric, cortex-policy-adapter). Mitigation, canon: KG1 lands the crate's complete API
  (including `OnSignal::Die`) in one commit, so KG2/KG3/KG5 touch only their callers.]
- `scripts/gate.sh --fast` per lane, then the full `scripts/gate.sh` once at the integration head, alone on the host:
  the gaps run saw a load-dependent failure with five lanes building at once (`opportunities.md:2500-2501`). Budget:
  one full run, wall time not measured in this draft [INFERENCE: tens of minutes].
  [REVISED: measured 24-40 min (`tasks/r51-baseline.md:21-40`); budget 40 min, >80 min is reviewed before a re-run.
  Note the fast tier does not run `-p axon-os`/`-p axon-intent`/`-p axon-jobctl` tests (`gate.sh:373-384`): the lane
  must run them itself; the full gate runs them through the manifest's `required` commands.]
- `v022_refusal_coverage.py`: seconds, in the fast tier (`gate.sh:332`).
- `v022_g01_mutations.py --only=<new rows>`: about six cells, each a rebuild of the mutated crate's dependents plus one
  test; budget ≤ 45 min, serial. Not `--changed-since --granularity=file`: KG1 touches `runtime.rs` and cortex files,
  which can trip the BROAD stop (`opportunities.md:2475-2477`) and cost hours (a broad lane run took 7260 s, `:2464`).
  [REVISED: budget ≤ 30 min (6 cells, see table); the `--changed-since` warning stands and is stronger than stated:
  the new crate is upstream of most runtime crates, so any impact selection that includes jobctl is BROAD.]

## Owned files

`crates/axon-jobctl/**` (new); `Cargo.toml` (members), `Cargo.lock`; `crates/axon-os/{Cargo.toml,src/runtime.rs}`;
`crates/axon-intent/{Cargo.toml,src/synth.rs}`; `crates/axon-cortex/{Cargo.toml,src/generate.rs,src/bin/cortex.rs,
tests/cli.rs,README.md}`, `docs/CORTEX.md:440` (generator row: "killed with its process group"); `crates/axon-reflex/
{Cargo.toml,src/lib.rs,tests/sidecar_teardown.rs}`; `scripts/gate.sh:835-842`; `governance/release-verification.json`;
`scripts/v022_refusal_coverage.py`, `scripts/v022_g01_mutations.py`, `scripts/g01_anchors.py` (re-pins);
`CHANGELOG.md`; `AXON-COMPLETENESS.json`/`.md` (integrator).
[REVISED: `crates/axon-reflex/**` dropped (KG4 excluded); added `crates/axon-os/src/cli.rs` (`:522`, `:590` import
path), `crates/axon-intent/src/main.rs` and an intent test file (KG5). `scripts/g01_anchors.py` is a checker, not a
registry: it is run, not edited. See Lane plan.]

Sequencing: KG1 first (G2 suite green on the extraction alone), then KG2 + KG3 together (KG2 without KG3 loses Ctrl-C
reach), KG4 independent after KG1.
[REVISED: KG1 → (KG2 + KG3 in one commit or one un-merged series) → KG5; KG5 is independent of KG2/KG3. KG4 excluded.
Per Main's lane plan, KG1 and AR1 (approval-residue) share one lane, KG1 first.]

## Lane plan

[REVISED: added for the integrator's disjointness check.]

Lane **A (jobctl+approval)** (one worktree, shared with approval-residue per Main and ReviewApproval): order KG1 → AR1 →
AR2 → AR3 → KG2+KG3 → KG5. AR edits no `Cargo.toml` (AR3 uses a test-local Runtime stub), so the axon-intent and
axon-os manifests stay KG1's; AR1 adds `Runtime::load_program` and changes `declared_effects`/`run_sandboxed`
parameters in runtime.rs after KG1's move, and touches neither `synth.rs` nor the G2 test files. KG2/KG3/KG5 touch no
file AR touches except the shared registries below, so they may instead run as a second worktree branched after
KG1's commit; either way not before KG1.

| Item | Files |
|---|---|
| KG1 | `crates/axon-jobctl/{Cargo.toml,src/lib.rs}` (new); `Cargo.toml` (members), `Cargo.lock`; `crates/axon-os/{Cargo.toml,src/runtime.rs:288-806 region,src/cli.rs:522,590}`; `crates/axon-intent/{Cargo.toml,src/synth.rs:204-308}`; `crates/axon-cortex/Cargo.toml` (dep added here so KG2 does not touch Cargo.lock again); `scripts/gate.sh:840` (one `-p`); `governance/release-verification.json` (one entry); `scripts/v022_refusal_coverage.py:3436-3469` (move/re-pin/re-reason) |
| KG2 | `crates/axon-cortex/{src/generate.rs:475-589,tests/cli.rs,README.md}`, `docs/CORTEX.md:440`; `scripts/v022_g01_mutations.py` (3 rows) |
| KG3 | `crates/axon-cortex/{src/bin/cortex.rs:326,tests/cli.rs}`; `scripts/v022_g01_mutations.py` (2 rows) |
| KG5 | `crates/axon-intent/{src/main.rs,tests/<new or acceptance.rs>}`; `scripts/v022_g01_mutations.py` (1 row) |
| all | `CHANGELOG.md`; `AXON-COMPLETENESS.json`/`.md` (integrator, last) |

Cross-spec contention (checked against the other three drafts' owned-file lists):
- `crates/axon-os/src/runtime.rs`: AR1 edits `:1199-1302` (`run_sandboxed`, `declared_effects`); KG1 edits `:288-806`.
  Disjoint hunks, same lane, KG1 first.
- `crates/axon-intent`: AR3/AR6 own `src/cli.rs`, `tests/acceptance.rs`, `README-axon-intent.md`; KG1 edits
  `synth.rs`, KG5 `main.rs`. KG5's test goes in a new file (e.g. `tests/signals.rs`), not `acceptance.rs`, to stay
  disjoint.
- `scripts/gate.sh`: KG1 adds `-p axon-jobctl` at `:840`; L3 (lsp) edits `:787-788` and `:360-362`; harness edits
  comments at `:1002`, `:1321`, `:1336`. Line-disjoint, so a three-way merge is clean; the integrator merges, no lane
  rebases another's gate.sh.
- `Cargo.toml`/`Cargo.lock`: only KG1 changes them among the four drafts [INFERENCE from their owned-file lists];
  integrator-owned — the lane commits them, the integrator re-runs `cargo metadata --locked` after merging.
- `scripts/v022_refusal_coverage.py`, `scripts/v022_g01_mutations.py`: AR and KG both add rows/exemptions. Each lane
  appends its rows in its own block at the end of `MUTATIONS`/`EXEMPT` (re-pins are in-place single-line edits), so
  merges do not conflict; row ids are taken from the integrator's next-free range, not chosen per lane.
- `CHANGELOG.md`, `AXON-COMPLETENESS.*`: integrator, last.

## Dropped on re-measurement

- `axon-cortex/src/runner.rs:3095` and `axon-psv/src/runner.rs:410`: the child can spawn nothing (`Exec` removed under
  the completion key, tested), so its kill already reaches the whole job, and with no descendant the unbounded drain
  joins (`runner.rs:3101-3102`, `psv runner.rs:416-417`) get EOF at once. Residual only if an operator registers a
  forking wrapper as `axon-test-local` (NH3).
- `axon-fabric/src/git_data.rs:396`: fixed `/usr/bin/git` with every helper-spawning path configured off; nothing to
  group-kill, and no fail-first test is possible without a different git.
- `axon-vm/src/firecracker.rs:223,448,466`: the VMM does not fork; a group kill adds nothing measurable, and moving it
  out of the terminal's group would cost `axon-vm`'s Ctrl-C reach. Noted, not built: the drain joins at `:478-480` are
  unbounded and would stall behind a non-exec wrapper.
[REVISED: reasoning reviewed and confirmed as consistent; all four drops are now [DECIDED by human] (NH3). One
correction of emphasis: "its kill already reaches the whole job" for runner.rs/psv holds only because the child
spawns nothing; it is a property of the registered binary, not of the kill path, which is why NH3 names the wrapper
residual.]

## Needs human

- **NH1 — generator leftovers after a normal exit.** Kill the generator's group once it has answered, or leave
  leftovers running (axon-os G2c leaves them and bounds only the drain)? Killing stops runaway background work; leaving
  them keeps a generator free to warm a local model server. Recommendation: leave them (consistent with axon-os), drain
  bounded.
  **[DECIDED by human, 2026-10-17: accepted as canon — leftovers are left alone, every pipe read is time-bounded.
  KG2 builds exactly that.]**
- **NH2 — reflex sidecar out of the caller's group.** Gains: `Drop` reaches the sidecar's whole tree. Loses: terminal
  Ctrl-C reaches it only if the host binary opts into forwarding (no in-tree host exists). Recommendation: do it,
  documented.
  **[DECIDED by human, 2026-10-17: EXCLUDED from this build, stays needs-human. KG4 cannot be done without this
  behaviour change (see KG4), so KG4 is excluded; remaining scope KG1, KG2, KG3, KG5.]**
- **NH3 — defence in depth on the dropped sites** (group kill in `run_limited`/`exec_axon_test` against a wrapper
  binary). Recommendation: no; registration pins one binary by sha256 and the operator should register `axon` itself.
  **[DECIDED by human, 2026-10-17: accepted as canon — no group kill at the four dropped sites.]**

## Non-goals

Timeouts where none exist (`runner.rs:458,1705,1779` `axon check`; `runner.rs:3426-3455` `axon-fabric submit`;
`main.rs:9499` `axon run --vm`); `synth.rs`'s sequential post-exit reads (`:295-302`), which can stall behind a
leftover on a normal exit (log to opportunities); the reflex `roundtrip` read with no time bound (`lib.rs:593-600`);
a subreaper.
[REVISED: add to the synth.rs opportunities entry: `run_bounded` polls `try_wait` without draining (`:274-288`), so a
child writing more than a pipe buffer blocks until the timeout — the T25 deadlock axon-os fixed (`runtime.rs:729-747`);
same file, same follow-up. Also non-goal: the reflex sidecar entirely (KG4 excluded).]

## Open questions / not verified

- Orphaning by `cli.rs:1587` was inferred from the shell measurement, not observed in a test run.
  [REVISED: unchanged; KG2 (a) observes it fail-first, which settles it.]
- Firecracker not forking and git `cat-file --batch` spawning no helper are from the tools' documented behaviour, not
  traced here. [REVISED: moot for this build — both sites dropped by human decision (NH3).]
- Gate and mutation wall times are estimates; the reviewer should get the last full `gate.sh` duration from a log.
  [REVISED: resolved — full gate 24-40 min measured at five heads (`tasks/r51-baseline.md:21-40`); written into
  Gates. Fast-tier and per-cell mutation times remain `[INFERENCE]`; the lane records its first measured run of each
  in its notes and the budgets are re-set from that.]
- [REVISED: added] `OnSignal` conflict policy (panic on a second, different mode) — resolved as canon above
  (engineering; a silent last-wins would change another binary's exit semantics).
- [REVISED: added] KG5's exact `axon-intent` verb/args for the test are `[INFERENCE]`; the builder confirms them in
  `intent cli.rs` before writing the test.
