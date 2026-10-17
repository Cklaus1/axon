# DRAFT — Whole-job cancellation for the remaining supervisors (KG)

**This draft MUST pass a Step 1 adversarial review before any build.** Nothing here is approved.

Source: `tasks/opportunities.md:2491-2493` ([P2] "Other supervisors kill only their direct child", deferred by the
gaps run). Base: `main` 71d6f503, worktree `gaps-int`. Every site was re-measured against this tree on 2026-10-17;
the listed line numbers had not moved. Reference pattern: `crates/axon-os/src/runtime.rs` `run_bounded_capped`
(G2, CHANGELOG top section) and `crates/axon-intent/src/synth.rs` `kill_group`.

Mission: where a supervisor's child can have descendants, a timeout or a teardown kills the whole job, a terminal or
service signal to the supervisor still reaches the job, and no pipe held by a leftover descendant can stall the
supervisor. Where the child cannot have descendants, say so with evidence and change nothing.

Shared acceptance: `scripts/gate.sh` shows no new failure against the 71d6f503 baseline; `python3 -B
scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay green, every new site in a protected file
carrying a killed mutation row or a reasoned exemption and every moved anchor re-pinned;
`crates/axon-core/tests/process_control_guard.rs` stays green; `AXON-COMPLETENESS.json` row "cancellation reaches the
whole job" (`:754-771`) gets the new evidence paths and `AXON-COMPLETENESS.md` is regenerated (integrator, last).

## Re-measurement (all six listed sites)

| Site | Spawns | Descendants plausible? | Kill path today | Verdict |
|---|---|---|---|---|
| `axon-cortex/src/generate.rs:552` | the operator's `cmd:PATH` generator (`:483`), any program, typically a script that runs a model CLI | **yes**: measured on this host, SIGKILL to a `#!/bin/sh` script running `sleep 4711` (dash, not exec'd) left the `sleep` running, re-parented | `child.kill()` + `wait()` at the deadline; no forwarding of signals to the supervisor; drains and stdin writer joined unbounded on the normal-exit path (`:564-566`) | **KG2, KG3** |
| `axon-reflex/src/lib.rs:610` | the caller's sidecar exe (`:555-560`), long-lived | yes for a wrapper or a sidecar that forks; the in-tree sidecar (`src/bin/sidecar.rs:16-17`) forks nothing | `Drop` kills the direct child only | **KG4** |
| `axon-cortex/src/runner.rs:3095` | the registered `axon-test-local` binary as `axon test … --completion-key-stdin` (`:2946-2960`), always | **no**: under a completion key `axon test` removes `Exec` from its effect ceiling before program code runs (`axon-core/src/main.rs:6463-6489`), proven by `axon-core/tests/psv_test_selection.rs:208` `holding_a_completion_key_spawns_nothing`; `axon test` itself spawns nothing (the only `Command::new` in `main.rs` are `:3317` rust-lld and `:9499` axon-vm, neither on the test path) | `child.kill()`; leader also has `PR_SET_PDEATHSIG` (`:3034-3056`) | dropped |
| `axon-psv/src/runner.rs:410` | pinned `axon test --completion-key-stdin` in the guest | **no**: `Exec` removed twice (`without_exec`, `:344`, and the interpreter as above); only production caller is the in-guest `axon-psv-runner` (`src/bin/axon-psv-runner.rs:89`, built by `scripts/build-guest-image.sh:229`) | `child.kill()`; `PR_SET_PDEATHSIG` + `NO_NEW_PRIVS` (`:345-374`) | dropped |
| `axon-fabric/src/git_data.rs:396` | `/usr/bin/git cat-file --batch` (`GIT_BIN`, `:29`; `:273-282`) | **no** [INFERENCE from git's documented behaviour]: env cleared, lazy fetch off, `protocol.allow=never`, hooks/fsmonitor off (`:33-66`); `cat-file --batch` without `--filters`/`--textconv` runs no helper | `Drop`: close stdin, `kill`, `wait` | dropped |
| `axon-vm/src/firecracker.rs:223,448,466` | the Firecracker VMM (`:255-261`), an absolute regular file (`FirecrackerBin::at`, `:783-793`) | **no** in production [INFERENCE: Firecracker is one multi-threaded process; the jailer named at `:361-363` execs it in place]; a forking wrapper is possible but a `sudo` wrapper leaves a root VMM that a user `killpg` cannot reach either | `kill` + `wait` in `LaunchGuard::drop` and at the sentinel grace / deadline | dropped |

Census (`.kill()` in non-test code): the six above plus `synth.rs:280` (already group-killed) — nothing unlisted.

## Design decision: one shared crate, `axon-jobctl` (engineering, decided)

The G2d machinery is process-global: a fixed registry of job groups (`runtime.rs:391`, 64 slots), a termination flag
(`:394`), and SIGINT/SIGTERM/SIGHUP handlers (`:383`, `:399-415`). A second copy in another crate would install a
second handler over the first and each would know only its own registry; `axon-fabric` already links `axon-os`,
`axon-cortex` and `axon-vm`. So the code lives once. A per-site copy is rejected for that reason, and because the
signal-safety details took 19 review findings in G2.

Where it lives: `axon-reflex` has no `libc` (`Cargo.toml` deps: serde_json, axon-cortex, axon-loop-contracts);
`axon-cortex` does not depend on `axon-os` and should not (it would pull `axon-audit` + `axon-attest` into the repair
loop for a process helper). A new leaf crate depending only on `libc` fits all callers. Cost, all paid in this change:
workspace member + `Cargo.lock`; `-p axon-jobctl` in the runtime clippy line (`gate.sh:835-842`, enforced by the
coverage check `:844-862`); a class-A entry in `governance/release-verification.json` (enforced by
`axon-core/tests/release_manifest.rs:56`); it enters the refusal-coverage protected scope automatically, because
`axon-os` and `axon-cortex` are in the measured protected set (`protected_crates()` today: attest, audit, cortex,
fabric, guest-init, guest-kernel, loop, loop-contracts, os, psv, vm, workspace-recipe). The guest build (`--locked`,
static musl) is unaffected: `axon-psv` does not gain the dependency.

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
Acceptance: behaviour-preserving, so the oracle is the existing G2 suite — `axon-os/tests/supervisor_signals.rs`,
the G2a/G2c/G2d tests in `axon-os/tests/legacy_adapter.rs`, and `synth.rs`'s bystander test
(`:689` `the_timeout_kill_reaches_only_the_childs_own_group`) — green
unchanged. Crate unit tests: a grandchild of a killed `Job` is gone (`/proc/<pid>/cmdline` oracle, pid file written by
the job, grandchild stdio to `/dev/null`), a bystander in the test's own group survives, `reap` frees the slot before
the wait. Refusal coverage: the axon-os exemptions at `v022_refusal_coverage.py:3438-3439` and `:3452-3468` move to the
crate's path; their reasons are re-derived for the crate's real caller set (axon-os, axon-intent, cortex generate,
reflex), not copied.

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
Acceptance (fail-first, all through the production `cortex repair --generator cmd:` path in `tests/cli.rs`):
(a) `hang.sh` writes `$$` and a background `sleep 4720`'s pid, then `wait`s; after the 700 ms deadline the run ends
code 24 and the grandchild is gone within 5 s (`/proc` + cmdline); today it survives. (b) a generator prints a body,
starts `sleep 60 &` without redirecting stdout, exits 0; `cortex repair` returns within 20 s (today ≥ 60 s); the test
kills the leftover itself with `libc::kill`. Mutations: `process_group(0)` removed in the crate → (a) red; `killpg`
replaced by `Child::kill` → (a) red; `join_within` replaced by plain `join` → (b) red.

## KG3 — `cortex` forwards termination signals to its jobs

Current: `bin/cortex.rs` (`main` at `:326`) installs no handler. Today terminal Ctrl-C reaches the generator because it
shares the foreground group; KG2 removes that, and SIGTERM to cortex (a CI `timeout`, a service manager) never
reached it. Change: `main` calls `forward_termination_signals(OnSignal::Die)` first. `Die` (new in the crate): the
handler SIGKILLs every registered group (as G2d), then sets the signal's disposition to `SIG_DFL` and `raise`s it, so
cortex dies by the same signal as today (no record to write, nothing to resume; a `Continue` flag would be ignored
outside a poll loop and swallow Ctrl-C). Already-ignored signals stay ignored (`nohup` keeps working).
Acceptance (fail-first): spawn `cortex repair` in its own process group with a `hang.sh` generator; once both pid
files exist, `libc::kill(cortex_pid, SIGTERM)`; cortex exits by SIGTERM (`ExitStatusExt::signal`) and the generator
and its grandchild are gone within 5 s; today both survive. Uses SIGTERM, not SIGINT: a background job of a
non-interactive shell (`gate.sh`'s `par`) starts with SIGINT ignored, which would make a SIGINT test vacuous.
Mutations: the `main` call deleted → red; `Die`'s re-raise removed → red (cortex keeps running).

## KG4 — The reflex sidecar is torn down as a job

Current: `SidecarBackend::spawn` (`lib.rs:554-577`) leaves the sidecar in the caller's group; `Drop` (`:608-613`) kills
the direct child, so a wrapper's real worker survives unless it exits on stdin EOF. Only caller in the tree:
`tests/principal_isolation.rs:156` (public API). Change: spawn via `Job::spawn`, `Drop` → `Job::kill` + `reap`. Doc
comment on `SidecarBackend` states that terminal Ctrl-C no longer reaches the sidecar group unless the host binary calls
`forward_termination_signals` (NH2). Acceptance (fail-first): a wrapper sidecar script records `$$` and a background
`sleep 4721` pid, then `exec cat >/dev/null`; dropping the backend leaves neither alive within 5 s; a bystander in the
test's group survives. Mutation: `Drop` back to `Child::kill` → red. Refusal coverage: none — `axon-reflex` is not in
the protected set (it is only a `[dev-dependencies]` entry of `axon-loop`, `Cargo.toml:21,30`; measured via
`protected_crates()`).

## Constraints every item inherits

- **No `kill` binary.** `process_control_guard.rs:88-94` fails on `Command::new("kill"|"/bin/kill"|"/usr/bin/kill"|
  "pkill"|"killall")` anywhere under `crates/`, tests included. Tests signal and clean up with `libc::kill`/`killpg`
  on typed pids; liveness via `/proc/<pid>/cmdline` (pid reuse) as in `axon-os/tests/supervisor_signals.rs:52-57`.
  Shell scripts may use the builtin (`process_control_guard.rs:22-25`). Never `killpg(0)`; tests that signal a
  supervisor spawn it with `process_group(0)` (`supervisor_signals.rs:59-84`).
- **Refusal coverage** (`PRIV_FORM`, `v022_refusal_coverage.py:4688-4703`): every new `.process_group(`, `libc::killpg(`,
  `libc::sigaction(`, `libc::pthread_sigmask(`, `.pre_exec(` in a protected file (the crate; `generate.rs` and
  `bin/cortex.rs` under the `_CTX` reason at `:1554`) gets a g01 row killed by the tests above or a checkable exemption.
- **g01 rows** (format as `v022_g01_mutations.py:7844-7847`): about six, listed under each item; package `axon-os`
  rows belong to the serial shard.

## Gates and cost budgets

- Lane loop: `cargo test --locked -p axon-jobctl -p axon-os -p axon-intent -p axon-reflex` and
  `cargo test --locked -p axon-cortex --test cli` (needs the interpreter built: `gate.sh:678-681`). Budget ≤ 15 min
  wall [INFERENCE]; CPU-bound, competes with any concurrent lane build.
- `scripts/gate.sh --fast` per lane, then the full `scripts/gate.sh` once at the integration head, alone on the host:
  the gaps run saw a load-dependent failure with five lanes building at once (`opportunities.md:2500-2501`). Budget:
  one full run, wall time not measured in this draft [INFERENCE: tens of minutes].
- `v022_refusal_coverage.py`: seconds, in the fast tier (`gate.sh:332`).
- `v022_g01_mutations.py --only=<new rows>`: about six cells, each a rebuild of the mutated crate's dependents plus one
  test; budget ≤ 45 min, serial. Not `--changed-since --granularity=file`: KG1 touches `runtime.rs` and cortex files,
  which can trip the BROAD stop (`opportunities.md:2475-2477`) and cost hours (a broad lane run took 7260 s, `:2464`).

## Owned files

`crates/axon-jobctl/**` (new); `Cargo.toml` (members), `Cargo.lock`; `crates/axon-os/{Cargo.toml,src/runtime.rs}`;
`crates/axon-intent/{Cargo.toml,src/synth.rs}`; `crates/axon-cortex/{Cargo.toml,src/generate.rs,src/bin/cortex.rs,
tests/cli.rs,README.md}`, `docs/CORTEX.md:440` (generator row: "killed with its process group"); `crates/axon-reflex/
{Cargo.toml,src/lib.rs,tests/sidecar_teardown.rs}`; `scripts/gate.sh:835-842`; `governance/release-verification.json`;
`scripts/v022_refusal_coverage.py`, `scripts/v022_g01_mutations.py`, `scripts/g01_anchors.py` (re-pins);
`CHANGELOG.md`; `AXON-COMPLETENESS.json`/`.md` (integrator).

Sequencing: KG1 first (G2 suite green on the extraction alone), then KG2 + KG3 together (KG2 without KG3 loses Ctrl-C
reach), KG4 independent after KG1.

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

## Needs human

- **NH1 — generator leftovers after a normal exit.** Kill the generator's group once it has answered, or leave
  leftovers running (axon-os G2c leaves them and bounds only the drain)? Killing stops runaway background work; leaving
  them keeps a generator free to warm a local model server. Recommendation: leave them (consistent with axon-os), drain
  bounded.
- **NH2 — reflex sidecar out of the caller's group.** Gains: `Drop` reaches the sidecar's whole tree. Loses: terminal
  Ctrl-C reaches it only if the host binary opts into forwarding (no in-tree host exists). Recommendation: do it,
  documented.
- **NH3 — defence in depth on the dropped sites** (group kill in `run_limited`/`exec_axon_test` against a wrapper
  binary). Recommendation: no; registration pins one binary by sha256 and the operator should register `axon` itself.

## Non-goals

Timeouts where none exist (`runner.rs:458,1705,1779` `axon check`; `runner.rs:3426-3455` `axon-fabric submit`;
`main.rs:9499` `axon run --vm`); `synth.rs`'s sequential post-exit reads (`:295-302`), which can stall behind a
leftover on a normal exit (log to opportunities); the reflex `roundtrip` read with no time bound (`lib.rs:593-600`);
a subreaper.

## Open questions / not verified

- Orphaning by `cli.rs:1587` was inferred from the shell measurement, not observed in a test run.
- Firecracker not forking and git `cat-file --batch` spawning no helper are from the tools' documented behaviour, not
  traced here.
- Gate and mutation wall times are estimates; the reviewer should get the last full `gate.sh` duration from a log.
