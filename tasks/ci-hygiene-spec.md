# ci-hygiene - spec (CH-1..CH-9, batch 4 Lane C)

**Status: PARTIAL 2026-11-05 on b4/ci-hygiene 1a34c386 (O-CI-01 stays open: CI is not green yet; O-CI-02 stays open): CH-1..CH-8
built and merged on `b4/merge`, CH-9 stamps applied. Integrated run 41992018913 at 43dda51d: Docs, Check & Test and Codegen
pass; the workspace job fails only on the privilege suites in O-CI-02 (NH-3 B). Build authorized by the human 2026-11-05
("use recommendations, build batch 4").**

**Step 1 review** (`history://AuditCI` CIA-1..5, `history://DevloopCI` CID-1..7; every finding re-checked at 9300d9fd by the drafter):

| finding | sev | disposition | where |
|---|---|---|---|
| CIA-1 wasm non-link skip also swallows real link failures | major | accepted (`main.rs:3849,3855,3889,3908,3933` prerequisite vs `:3947,3965,3980` link-failed re-read; today `cli_run.rs:35188-35194` is an uncounted `eprintln` skip) | CH-5 step 2, §8 |
| CIA-2 M4830 has no ATTACK marker; run refuses | major | accepted (`v022_g01_mutations.py:8820-8828` drift refusal re-read) | CH-4 step 3, §4, §7 |
| CIA-3 "exit = frames" claims left behind | minor | accepted (`android_lifecycle.sh:13,81,90,92`, `lifecycle.ax:46`, `R14-mobile-targets.md:546` re-read) | CH-4 steps 2, 4 |
| CIA-4 `stdout.contains("SKIP") \|\|` bypasses the exit assert | minor | accepted (`cli_run.rs:1004`; the assert lives in `harness_skipped` `:85-95`) | CH-4 step 3 |
| CIA-5 key fetch outside retry, no pipefail | minor | accepted (`ci.yml:202-203` re-read; unspecified `run:` shell is `bash -e {0}`) | CH-7 |
| CID-1 every CI run is cold (cache saved only on success) | major | accepted (`run…log:143,3412,6510` `Cache not found`) | §6 S4 |
| CID-2 no non-`main` CI path except PRs | major | accepted (`ci.yml:3-7`; `ios.yml` has `workflow_dispatch`) | CH-2 fix, §6, §7 |
| CID-3 unmasked failures unbounded | major | accepted; the `#[ignore]` route made needs-human (NH-3), never automatic | CH-2 risk, §5.1, §8, NH-3 |
| CID-4 S1 thrashes the shared target dir | minor | accepted | §6 S1 |
| CID-5 re-run triggers unstated | minor | accepted | §6 |
| CID-6 N=10 rationale invalid (1/8 is one event) | major | accepted; rationale rewritten | NH-1 |
| CID-7 S4 "+10-20 runner-min" guessed | minor | accepted | §6 S4 |

Accepted 12, rejected 0.

Subject: the GitHub Actions workflows (`.github/workflows/ci.yml`, `ios.yml`), the tests and harness scripts the CI failures
implicate (`crates/axon-core/tests/{cli_run.rs,harness_integrity.rs}`, `scripts/android_lifecycle.sh`,
`examples/mobile/lifecycle.ax`), three `cfg` lines in `crates/axon-core/src/{lib.rs,outcome.rs,interp.rs}`, and stamp text for
stale entries in `tasks/opportunities.md` and `tasks/ceilings-spec.md`. Base: `main` 9300d9fd (worktree
`/home/cklaus/projects/axon-lanes/b4`, branch `b4/specs`). Every anchor below was read at 9300d9fd.

Author: `CiHygiene` (2026-11-04). Evidence was gathered with read-only `gh` (`gh run list`, `gh run view --log-failed`, `gh api
.../runs`, `.../jobs`, `.../rulesets/26386102`). Nothing was re-run or changed remotely. Logs and probes are in
`/var/tmp/r51-tmp/CiHygiene/` (`run41875787424.log` = CI on 9300d9fd, `r<id>.log` = the seven runs before it, `ios.log`,
`runs.tsv` = all 303 CI runs). Reference binary: `/home/cklaus/projects/axon-lanes/.doc-build/b4-draft/debug/axon` (`axon 0.1.0
(9300d9fd)`). It is an interpreter-only build, so no codegen probe was possible. No cargo build, no model call, no GPU, $0 spent.

## 0. Source entries

| item | entry | sev |
|---|---|---|
| CH-1..CH-7 | CI workflow `CI` concluded `failure` on **all 303 runs** (2026-04-30 05:43 → 2026-11-04 17:15; `runs.tsv`, `cut -f2 \| sort \| uniq -c` = `303 failure`). `gh run list --status success` is `[]`. **CI has never passed.** iOS workflow: 16 failures + 1 cancelled, never passed | high |
| CH-8 | wasm32 dead code found while diagnosing CH-1 | low |
| CH-9 | stale entries: O005 (`tasks/opportunities.md:98`), O007 (`:206`), O012 (`:317`), and the `tasks/ceilings-spec.md:3` status line | low |

Job-level history (sampled every 15th run plus the newest 6): `Docs & spec hygiene` passes every sampled run since 2026-09-19.
`Check & Test`, `Test the whole workspace` and `Codegen build + tests` fail on every sampled run. The newest 7 runs
(ebbb4e02..9300d9fd) fail the **same five tests** (table §2). One run (8b1c83b1) failed its codegen job at `apt-get install
llvm-23` instead.

**"REDTEAM FAILED: blocking deploy" is not a failure.** It is stdout from `examples/goals/redteam-goal.ax:49`, which
`interp::tests::goal_demos_pure_outcomes` (`interp.rs:9552`, case `("redteam-goal.md", VERIFY_FAILED_EXIT_CODE)` at `:9566`) runs
and *expects* to exit 3. The next log line is `test interp::tests::goal_demos_pure_outcomes ... ok` (`run41875787424.log:1526-1527`),
and the lib suite ends `1121 passed; 0 failed` (`:1956`). It shows up because the interpreter's `println` writes straight to fd 1,
so libtest's capture never sees it.

### Out of scope (logged, not dropped)

| what | why not here | where it goes |
|---|---|---|
| The REDTEAM line and other interpreter stdout leak past libtest capture into CI logs | Only log noise. Nothing is graded on it | integrator: new O-entry (low) |
| `wasm_unknown_interp_builds.sh:44` writes to a fixed `/tmp/wasm_unknown_check.err` | Shared-tmp hygiene. No failure was observed | integrator: new O-entry (low) |
| `crates/axon-guest-kernel/src/mmds.rs:58` comment still says "use open policy". The code at `:59-61` is deny-all | A comment only | folded into the O007 stamp (CH-9) |
| No `wasmtime` on the CI runner, so the pure-module RUNNABLE command in CH-5 stays unrun in CI | Installing it would add coverage. It is not needed to make CI green | integrator: O-entry beside CH-5 |
| Authoring `tools/ios-parity-harness/run_on_sim.sh` (needs a macOS host and Xcode) | Feature work. NH-2 decides the scope | integrator: new O-entry (R14 iOS remainder) |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | CH-2, CH-4, CH-1, CH-3 | CH-2: `cargo test` stops at the first failing test binary, and later steps never run. So CI's fmt, clippy and claims steps, 26/35 axon-core test binaries (Check & Test) and every crate after axon-core (workspace) have measured **nothing** while the job names claim them. That is a claimed property with no enforcement. CH-4: a real regression since 2026-08-31 (`examples/mobile/lifecycle.ax` exits 1, not 3). The local gate never runs it, because the harness skips on a missing NDK *before* the host-only half. CH-1, CH-3: environment differences that make CI red on correct code |
| P1 | CH-5, CH-6 | CH-5: a missing prerequisite (wasm32 `libaxon_rt.a`) is read as a result, because a path substring fooled the skip detector. CH-6: the iOS job calls a script that was never authored |
| P2 | CH-7, CH-8, CH-9 | infra flake retry, wasm32 dead-code `cfg`s, stamps |

## 2. Failure classification (CI run 41875787424 on 9300d9fd, and the 6 runs before it)

| job | failing test / step | class | evidence line | item |
|---|---|---|---|---|
| Check & Test | `interp_compiles_for_wasm32_unknown_unknown_r7c` (`wasm_unknown_interp_builds.sh`) | env: CI's `RUSTFLAGS=-D warnings` | `error: constant WRITABLE is never used` … `-D dead-code implied by -D warnings` (`run…log:6180,6199`) | CH-1 (+CH-8) |
| Check & Test | `wasm_interpreter_evals_identically_to_native_r7c` (`wasm_browser_interp_parity.sh`) | same | `cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release failed` (`:6192-6211`) | CH-1 |
| Test workspace (and the masked Check & Test / codegen runs) | `harness_integrity::a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit` | env: the runner has no git identity | `git commit-tree HEAD^{tree} -m orphan returned non-zero exit status 128` (`:3243`) | CH-3 |
| Codegen | `android_lifecycle_adapter_r14` (`android_lifecycle.sh`) | **real bug**, exposed because the runner has an NDK | `FAIL — (A) lifecycle exit = 1, expected 3` (`:9251`) | CH-4 |
| Codegen | `a_wasm_runnable_label_is_only_printed_for_a_module_that_runs` (`cli_run.rs:35250`) | missing prerequisite misread as a result | `wasm runtime not found: libaxon_rt.a for wasm32-unknown-unknown` … `println.wasm: undefined symbol: puts` (`:9239-9241`) | CH-5 |
| Codegen (8b1c83b1 only) | step `Install LLVM 23` | infra flake | `Temporary failure resolving 'apt.llvm.org'` → `E: Unable to locate package llvm-23` (`r41663312668.log`) | CH-7 |
| iOS (all 16 failures) | step `Run compute parity on the iOS Simulator` | harness never authored | `bash: run_on_sim.sh: No such file or directory`, exit 127 (`ios.log:756`). `tools/ios-parity-harness/` holds only `README.md` | CH-6 |
| all three test jobs | everything after the first failing binary | masking (fail-fast) | `Running` lines per job: 25 / 12 / 13. The last one is `harness_integrity` / `cli_run` / `cli_run` | CH-2 |

Why the local strict gate stays green on the same commits: the gate does not set a global `RUSTFLAGS` (it only scopes mold to the
host, `gate.sh:138`). The dev host has a global git identity, has no NDK, and has `target/wasm32-unknown-unknown/debug/libaxon_rt.a`
(observed in `/home/cklaus/projects/axon/target`). Each failure is one of these environment differences. CH-4 is the exception:
it is a real defect that the environment *hid* locally.

## 3. Items

### CH-1 (P0) CI's global `RUSTFLAGS` replaces the committed wasm32 flags and fails both wasm harnesses

**Anchors.** `ci.yml:9-11` sets `RUSTFLAGS: -D warnings` for every job. `ci.yml:134-135` and `:194-195` set `RUSTFLAGS: ""`.
`.cargo/config.toml` sets `[target.wasm32-unknown-unknown]` and `[target.wasm32-wasip1]` to `rustflags = ["-C",
"link-arg=-zstack-size=67108864"]`. Cargo uses exactly **one** rustflags source: `CARGO_ENCODED_RUSTFLAGS`, else `RUSTFLAGS`, else
`target.<triple>.rustflags`, else `build.rustflags`. So any set `RUSTFLAGS` (the empty string included) drops the committed 64 MiB
wasm stack from every wasm build in every CI job `[INFERENCE: cargo reference "build.rustflags"; not probed on a runner]`. In
Check & Test it also turns wasm32-only dead-code warnings into errors.

**Reproduction.** The same tests **pass** in the workspace job, which runs with `RUSTFLAGS=""`:
`interp_compiles_for_wasm32_unknown_unknown_r7c ... ok` (`run…log:2868`), `wasm_interpreter_evals_identically_to_native_r7c ... ok`
(`:3124`). They fail in Check & Test (`:5854`, `:6153`) with the errors quoted in §2. Not reproduced locally: that needs a cargo
check, which is out of this drafter's budget.

**Fix.** In `ci.yml`, replace `RUSTFLAGS: -D warnings` (`:11`) with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS: -D warnings`.
This is the same host-scoping `gate.sh:137-138` uses: host warnings stay fatal, and wasm targets keep their committed flags. In
`:134-135` and `:194-195`, replace `RUSTFLAGS: ""` with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS: ""`. Update the comments
at `:122-127` to name the variable. Clippy keeps its explicit `-- -D warnings` (`:100`, `:115`).

**Options considered.** (a) `cfg` fixes alone (CH-8): CI would turn green, but the wasm stack-flag drop would stay hidden. (b) Make
the harness force `-D warnings` locally: this changes what the local gate measures, and it fingerprint-thrashes the shared wasm32
build dir. (c) **Recommended: host-scope the flag, plus CH-8.**

### CH-2 (P0) Fail-fast hides most of CI

**Anchors.** `ci.yml:83`, `:164` and `:238` run `cargo test` with no `--no-fail-fast`. The steps after the test step (`:88-93`
claims gate, `:95-96` fmt, `:99-100`, `:109-115` clippy) have no `if:`, so a test failure skips them. Count: axon-core has 35
integration test binaries. 26 sort after `cli_run.rs` and 21 after `harness_integrity.rs`, and none of those ran in their job.

**Fix.** Add `--no-fail-fast` to all three `cargo test` lines. Give the four steps at `:88`, `:95`, `:99` and `:109` an `if: ${{
!cancelled() }}`, so fmt and clippy report even when tests fail. The claims gate already fails loudly when the binary is absent
(`CLAIMS_GATE_REQUIRE=1`, `:90`). `[REVISED: CID-2]` Add `workflow_dispatch: {}` to `ci.yml` `on:` (`:3-7`) in the same commit,
so a branch can be run with `gh workflow run ci.yml --ref <branch>` once it is on `main`.

**Risk, stated.** The first `--no-fail-fast` run will show test binaries CI has never run. They pass in the local strict gate, but
local is not the runner. No new failure may be skipped. `[REVISED: CID-3]` Triage rule, by each failure's first error line:
- **env** (the test depends on the runner's config): fix the test, the CH-3 way, never the runner;
- **missing prerequisite**: if CI can provide it in at most one step, add that step; otherwise the test records a countable
  `note_harness_skip` *only* for the named prerequisite text (the CH-5 rule), and every other outcome stays a failure;
- **real bug**: fixed here only inside Lane C files (§7); outside them, an O-entry, and the job stays red. Marking it
  `#[ignore]` turns a failure into a non-run, so it is never done by the builder: NH-3.

Caps: at most 5 such fixes in this lane, and at most 3 draft-PR CI iterations after S1. Past either cap, stop, list the remaining
failures with their class, and escalate to the integrator instead of growing the lane.

### CH-3 (P0) `harness_integrity` needs a git identity the runner lacks

**Anchors.** `harness_integrity.rs:50-60`: `git()` passes `-c user.name=t -c user.email=t@example`. The `not-ancestor` case at
`:1840` runs `subprocess.run(['git','commit-tree','HEAD^{tree}','-m','orphan'],…,check=True)` through `py()` (`:116-126`), with no
identity.

**Reproduction** (`/var/tmp/r51-tmp/CiHygiene/gitid/r`, git 2.53.0):
```
GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git commit-tree 'HEAD^{tree}' -m orphan   -> exit=128
  (fatal: unable to auto-detect email address (got 'root@gpumaster.(none)'))
... git -c user.name=t -c user.email=t@example commit-tree ...                              -> exit=0
git commit-tree ... (host global config)                                                    -> exit=0
```

**Fix.** At `:1840`, change the argv to `['git','-c','user.name=t','-c','user.email=t@example','commit-tree','HEAD^{tree}','-m','orphan']`.
That is the file's own convention (`:54`). Do **not** set a global identity in CI, because that would hide the next non-hermetic
test.

**Fail-first test.** The existing test, run as `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 cargo test -p axon-core
--no-default-features --test harness_integrity a_status_file_is_accepted_only_as_the_joined_evidence_for_the_freeze_commit`.
Expected: it fails before the fix with exit status 128 in the panic text, and passes after.

### CH-4 (P0) `android_lifecycle` (A): a real regression the local gate cannot see

**Anchors.** `examples/mobile/lifecycle.ax:46-47` returns `frame` (3) from `main`. Commit b17e2bca (2026-08-31, "a value falling
out of `main` is an answer, not a process status") maps a returned reserved code to exit 1. `android_lifecycle.sh:56` skips on a
missing NDK, and `:67` (`require_codegen_axon`) skips on an interp-only axon. Both come **before** the host-only half (A)
(`:75-96`), which needs neither. `:78` discards stderr (`2>/dev/null`), which hid the diagnosis. `cli_run.rs:985-1017` treats the
SKIP as a non-result (`note_harness_skip`, `:1006`).

**Reproduction** (reference binary, `:78`'s exact input):
```
$ printf 'tick\ntick\nsuspend\nresume\ntick\nteardown\n' | axon run examples/mobile/lifecycle.ax
... on_destroy: teardown; frames=3 suspends=1
axon: `main` returned 3, and 3 is RESERVED — ... Exiting 1 instead; ...
exit=1
$ AXON=<ref> bash scripts/android_lifecycle.sh                       -> android_lifecycle: SKIP — Android NDK not found   (exit 0)
$ ANDROID_NDK_HOME=<fake dir> AXON=<ref> bash scripts/android_lifecycle.sh -> SKIP — $AXON ... is an interpreter-only build
```
The CI runner has an NDK and a codegen axon, so it reaches (A) and fails (`run…log:9251`). The full harness path with a codegen
binary was not run here, because no codegen build was available `[INFERENCE: :9251 is that path]`.

**Fix.**
1. `lifecycle.ax:46-47`: `main` returns `0`. The teardown transcript line `frames=3 suspends=1` is the witness. A reserved code
   used as a witness is exactly what b17e2bca forbids, and `exit(3)` would claim the verify-failed class.
2. `android_lifecycle.sh`: run (A) right after `$AXON` is resolved (`:60-66`) and `WORK`/`trap` are set (`:69-70`). Move
   `NDKBIN=… || skip` (`:56`) and `require_codegen_axon` (`:67`) to after (A), and make the skip text `Android NDK not found ((A)
   passed above)`. (A) expects exit `0`. Capture its stderr to `$WORK/lc.err` and print it on FAIL. The SKIP stays the final line,
   so `tail_says_skipping` is unchanged. `[REVISED: CIA-3]` Rewrite the exit-is-frames text at `:13`, `:81`, `:90` and `:92`: the
   witness is the `frames=3 suspends=1` transcript line plus exit 0. `:92` keeps its literal `  OK (A) ` prefix.
3. `cli_run.rs:1004-1008`: `[REVISED: CIA-4]` replace the condition `stdout.contains("SKIP") || harness_skipped(…)` with
   `harness_skipped(&out, &stdout, &stderr, &script)`, so a non-zero exit always reaches the "a failure is never a skip" assert.
   Inside the skip branch, before `note_harness_skip`, add `assert!(stdout.contains("OK (A)"), "ATTACK: android_lifecycle
   skipped before the host half (A) ran: …")` (`[REVISED: CIA-2]` the message carries M4830's marker). Update the comment at
   `:991-992`.
4. `[REVISED: CIA-3]` The `lifecycle.ax:46` comment and `governance/specs/R14-mobile-targets.md:546` (`exit=frames`) now name the
   transcript witness and exit 0.

Effect: (A) runs in every `cargo test` (interp only, under 1 s), including `--no-default-features` and the local gate.

**Fail-first test.** `android_lifecycle_adapter_r14` with assertion 3 and script change 2 applied, and change 1 not yet applied.
It FAILs locally with no NDK (`(A) lifecycle exit = 1, expected 0`), and passes after change 1.

### CH-5 (P1) The wasm runnable-label test misreads a missing runtime as a result

**Anchors.** `cli_run.rs:35189` reads "linked" from `err.contains("wasm:")`. The linker error path
`/tmp/axon_wasm_label_<pid>/println.wasm: undefined symbol` contains `wasm:`, so a module that never linked is graded at
`:35203-35206`. The real linked marker is a line starting `wasm: ` (`main.rs:3655`, `:3662`, `:3670`). The runtime absence is
`wasm runtime not found:` (`main.rs:3908`). `:35224` prints "node is not on PATH" whenever the host command is missing from the
label. But node **is** on the runner: `wasm_browser_interp_parity.sh:41-42` would otherwise have skipped, and that job got past it
(`run…log:6191`). The codegen job never builds the wasm32 runtime.

**Fix.**
1. `ci.yml` codegen job: after `cargo build (codegen)` (`:232-233`), add the step `cargo build -p axon-rt --target
   wasm32-unknown-unknown`. It produces `target/wasm32-unknown-unknown/debug/libaxon_rt.a`, which is the first path searched
   (`run…log:9239`).
2. `cli_run.rs:35188-35195`: `linked = err.lines().any(|l| l.starts_with("wasm: "))`. `[REVISED: CIA-1]` When it is false:
   if `codegen_absent(&err)` (`:188`) `|| err.contains("wasm link unavailable:") || err.contains("wasm runtime not found:")`,
   call `note_harness_skip(&format!("wasm runnable label [{tag}]: prerequisite missing: {first line of err}"))` (countable, fatal
   under `AXON_HARNESS_STRICT=1`) and `continue`. Otherwise `fails.push(format!("[{tag}] the module did not link with its
   prerequisites present: {err}"))` and `continue`. Today both cases are an uncounted `eprintln` skip (`:35188-35194`), and a real
   link failure fails only by accident (`wasm:` in the rust-lld path). After this, a real link regression is a failure by rule.
3. `:35223-35225`: say which case it was, either "node is not on PATH" or "the label printed no host command".

**Reproduction.** CI log only (`run…log:9235-9242`). Locally the test passes, because `libaxon_rt.a` is present. Fail-first: in a
scratch worktree whose `target/wasm32-unknown-unknown/` lacks the runtime, the pre-fix test fails with `[println] the label does
not name the host import`. Post-fix it passes and appends one line to `target/harness-skips.log`.

### CH-6 (P1) iOS: the simulator-parity step calls a script that does not exist

**Anchors.** `ios.yml:151-166`. `[ -d tools/ios-parity-harness ]` holds (the directory has `README.md` only), so `bash
run_on_sim.sh` exits 127. The `else` branch (`:162-165`) is itself a warning-only skip. `README.md` "Honest status" item 2 says
`run_on_sim.sh` "was NOT created". All 16 failed runs (2026-06-26 → 2026-11-03) fail at this one step, and every earlier step
passes. The job name `:41` claims "simulator parity". Recommended fix (NH-2 B): delete the step `:146-166`, rename the job `:41`
to `iOS cross-build + oracle + Swift wrapper (macOS) — simulator parity NOT run (run_on_sim.sh not authored)`, and point the
header (`:15-17`) at the O-entry. The `window_clear` smoke step (`:168-177`) will then run for the first time. Locally, with the
reference binary: `ios-window-clear rendered 1 frame(s)`, exit 1 (`main` returns `frames`). The step tolerates the exit (`|| true`)
and greps the line, so it should pass `[INFERENCE: not run on macOS]`.

### CH-7 (P2) LLVM apt flake

`ci.yml:200-209`: `[REVISED: CIA-5]` give the step `shell: bash` (which runs `-eo pipefail`; the default `bash -e` has no
pipefail), and wrap the whole block (key `wget | tee`, sources line, `apt-get update`, `install`) in a 3-attempt loop with 20 s
sleeps that prints `apt attempt k failed` on each retry. After the third failure it still fails the job. Seen in 1 of 8 recent
runs (8b1c83b1, where DNS failed inside `apt-get update`, which printed only `W:` lines and exited 0).

### CH-8 (P2) wasm32 dead code (correct on host, warns on wasm32)

`lib.rs:622`: `const WRITABLE` is used only inside `#[cfg(unix)]` (`:627-636`). Give it `#[cfg(unix)]`. `outcome.rs:81-82`:
`Channel.fd` is read only under `#[cfg(unix)]` (`:177-184`). Add `#[cfg_attr(not(unix), allow(dead_code))]` to the field.
`interp.rs:7974` `with_run_meters_opt`: both callers (`:3026` in `on_deep_stack`, `#[cfg(not(target_arch = "wasm32"))]` at `:3016`,
and `:4282` in `run_suspendable_values_inner`, cfg `:4253`) are non-wasm. Give it `#[cfg(not(target_arch = "wasm32"))]`. All three
date from 2026-11-01/02 (db854cdb, 5814a792, e05862d9). The host build does not change. `[REVISED: CBD-2 cross-spec]` Lane A
(Containment) also edits `lib.rs` and `outcome.rs`, and its new `#[cfg(unix)]` `installed_fd()` reads `Channel.fd`. The hunks
are disjoint, and CH-8 lands first; see the §7 ownership table and merge order.

### CH-9 (P2) Stamps for the integrator (exact text, appended under each heading)

- **O005** (`opportunities.md:98`): `` `[FIXED 29802068 (fmt sweep) + 9520726f (fmt in gate.sh at --all): ci.yml:95-96 runs `cargo fmt --all -- --check`; it exits 0 with no output on 9300d9fd (2.6 s, nightly-2026-07-11). Open - CI has never reached that step: it follows `cargo test` (ci.yml:82-83), which fails every run; ci-hygiene CH-2]` ``
- **O007** (`:206`): `` `[FIXED 2cdb481a (T48) + 25b7dea1: every miss path denies - mmds.rs:59-61 no boot_params, :71-73 no cmdline ptr, :111-113 axon.policy= absent, :137-139 base64 decodes to nothing, read_policy :173-179 !POLICY_READY, mmds_parse.rs:186-191 absent/non-array allowed_effects, :215-217 unterminated array (all EffectSet(0)); statics mmds.rs:47 EffectSet(0), enforce.rs:47 0; prerequisite (1) delivered: axon-vm firecracker.rs:802-807 appends axon.policy=<base64>; tests crates/axon-vm/tests/guest_policy_parse.rs:46,167. Nit - mmds.rs:58 comment still says "open policy"]` `[INFERENCE: not booted in a guest]` ``
- **O012** (`:317`): `` `[FIXED 17f2be9b (T46): ast::walk_expr (ast.rs:702, moved from codegen/mod.rs); per-call tier refusal codegen/mod.rs:1391-1412 (E0910); tests cli_run.rs:703 native_guard_tier_e0910, :10997 build_refuses_per_call_ai_tier_e0910_f062]` ``
- **`tasks/ceilings-spec.md:3`** becomes: `**Status: BUILT 2026-11-02 (CE-1..CE-8, CE-10 on b2/ce via b2/int 785a832f, ICE-1..3 fixed through 9c828162; CE-9 built as tasks/guest-fs-spec.md GF-1..GF-5 on b3/int, merged 05aefe5b; NH-C9 deferred - a question, not an item).**`

## 4. g01 mutation rows (block M4830-M4849)

| id | file / find → replace | killing test | crate, args |
|---|---|---|---|
| M4830 | `scripts/android_lifecycle.sh`: `echo "android_lifecycle: (A) host_await lifecycle round-trip…"` → `skip "Android NDK not found"` (the old order: skip before (A)) | `android_lifecycle_adapter_r14` (CH-4 assertion 3: a skip without `OK (A)` fails) | `axon-core`, `--no-default-features --test cli_run` |

`[REVISED: CIA-2]` ATTACK marker, added to `scripts/v022_attack_markers.py` beside `'M4781'` (`:4165`):
`'M4830': r'ATTACK: android_lifecycle skipped before the host half \(A\) ran',`. Without it the run refuses (`ATTACK_MARKERS drift`).

There are no other rows, because no other change has a test a source mutation can fail. CH-1, CH-2, CH-6 and CH-7 edit YAML, which
no cargo test reads. CH-3 and CH-5 change test code only. CH-8 `cfg`s: removing one yields a wasm32 *warning*, which no gate turns
fatal after CH-1. M4831-M4849 stay unused.

## 5. Acceptance

1. A CI run on the change, from a **draft PR** from `b4/ci-hygiene` to `main` (`[REVISED: CID-2]`; never verified by pushing to
   `main`), shows none of the five §2 test failures. The fmt, clippy and claims steps report. Every test job prints `Running`
   lines for all binaries. Every further failure is triaged under the CH-2 rule within its caps. None is skipped. Green on all four
   jobs is the goal. If unmasked failures fall outside this lane, acceptance reads "no unclassified failure, each out-of-lane one
   has an O-entry", and NH-3 decides whether `main` may stay red on them.
2. Local: CH-3 and CH-4 fail-first tests fail before the fix and pass after. CH-5's post-fix test passes and logs one skip line in
   the scratch worktree. M4830 is killed with its marker matched (`v022_g01_mutations.py --only M4830`).
3. iOS: after CH-6, the next path-matching push runs the iOS workflow to completion, or a later step fails with its own message.
   It no longer ends in a 127 on a missing file.
4. `scripts/gate.sh --strict` passes on the build tip.

## 6. Gate stages and cost

| stage | what | cost |
|---|---|---|
| S0 | CH-3 and CH-4 fail-first runs on the pre-fix tree | ~5 min (two targeted test builds) `[INFERENCE]` |
| S1 | hermetic replica: `CARGO_TARGET_DIR=/var/tmp/r51-tmp/<lane>/ci-target GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-D warnings" cargo test --no-default-features -p axon-core --no-fail-fast`, then the same with `--workspace` and the flag empty. `[REVISED: CID-4]` The scratch target dir keeps the shared `target/` fingerprint for S3. This surfaces CH-2's unmasked failures for the two no-LLVM jobs. It does **not** cover the codegen job (NDK + LLVM 23), which only S4 checks | cold build + tests, twice: 30-45 min `[INFERENCE]`; timed and recorded |
| S2 | g01 `--only M4830` | one cell, ~5 min `[INFERENCE]` |
| S3 | `scripts/gate.sh --strict` | ≈ 44 min (2663 s, batch 3) |
| S4 | `[REVISED: CID-2]` draft PR `b4/ci-hygiene` → `main`; each push is one run. iOS via `gh workflow run ios.yml --ref b4/ci-hygiene` (`workflow_dispatch` exists) | `[REVISED: CID-1, CID-7]` measured ≈ 14 runner-min per run, wall 5-7 min (8 runs: 13, 16, 17, 11, 15, 14, 14, 14), all **cold**: `actions/cache` saves only on a green job and none has ever been green (`Cache not found`, `run…log:143,3412,6510`). With `--no-fail-fast` unmeasured; record the first run's step times. At most 3 runs (CH-2 cap), < 60 runner-min. iOS ≈ 4.6 min macOS. PUBLIC repo: $0 |

`[REVISED: CID-5]` Re-run triggers: S3 only after a non-YAML change (CH-3/4/5/8 or a CH-2 triage fix); S4 on every push to the PR
branch; a YAML-only change (CH-1/2/6/7) needs S4 only. Cortex receipt regress: not needed, because `crates/axon-cortex/src` is
untouched ($0). Model spend: $0.

## 7. Sequencing and lanes

Lane C files: `.github/workflows/{ci,ios}.yml`, `harness_integrity.rs:1840`, `cli_run.rs:985-1017` and `:35164-35251`,
`scripts/android_lifecycle.sh`, `examples/mobile/lifecycle.ax`, `governance/specs/R14-mobile-targets.md:546`,
`scripts/v022_attack_markers.py` (M4830 marker), and the CH-8 lines (`lib.rs:622`, `outcome.rs:82`,
`interp.rs:7973`). `[REVISED: CBD-2 cross-spec]` The earlier claim here, that Containment edits neither `lib.rs` nor
`outcome.rs`, was false. Shared-file ownership (batch 4):

| file | Lane C (this spec) | Lane A (Containment) |
|---|---|---|
| `crates/axon-core/src/interp.rs` | `:7973` (CH-8 `cfg`) | `:1585`, `:5941-5953` (`provenance_write_permitted`), `:8067-8162` |
| `crates/axon-core/src/lib.rs` | `:622` (CH-8 `cfg`) | the `path_scope` module declaration in the module list `:42-75` |
| `crates/axon-core/src/outcome.rs` | `:81-82` (CH-8 `cfg_attr` on `Channel.fd`) | `installed_fd()` after `:110` (`#[cfg(unix)]`, reads `Channel.fd`) |
| `crates/axon-core/src/main.rs` | `:3849-3980`, read-only: CH-5 matches its message texts; no edit | outcome fd (Lane A owner) |
| `crates/axon-core/tests/cli_run.rs` | `:985-1017`, `:35164-35251` | appends at the tail only |
| `.github/workflows/{ci,ios}.yml`, `scripts/android_lifecycle.sh`, `crates/axon-core/tests/harness_integrity.rs` | Lane C only | none |

Merge order: Lane C's CH-8 `cfg` commit first, then A, then B. Once A's `installed_fd()` lands, `Channel.fd` is read on unix
through two paths. The CH-8 `cfg_attr(not(unix), allow(dead_code))` is still correct there, because no non-unix reader exists.
All hunks are disjoint.

CH-4 and CH-8 change what the `axon` binary is built from, so like Lane A
they land **before** the genviab PREREG freeze, or the freeze recomputes the pin. The CI-only items (CH-1, CH-2, CH-5 step 1,
CH-6, CH-7) can land at any time. Order: S0 → CH-3, CH-4, CH-5, CH-8 → S1 → S2 → one `ci.yml` commit (CH-1, CH-2 incl.
`workflow_dispatch`, CH-5 step 1, CH-7) → CH-6 → S3 → S4 on the draft PR (≤ 3 iterations) → merge → NH-1 count starts.
Integrator-owned edits: the CH-9 stamps, the M4830 row in `v022_g01_mutations.py`, plus new O-entries for the §0 out-of-scope rows,
the iOS remainder, and any out-of-lane failure CH-2 unmasks.

## 8. Engineering decisions (canon after Step 1)

- `[REVISED: CIA-1]` A missing prerequisite, named by its own text (`codegen_absent`, `wasm link unavailable:`, `wasm runtime not
  found:`), is a *countable* non-result (`note_harness_skip`), never an `eprintln` and never a pass. **Any other non-link is a
  failure.** A non-zero harness exit stays a failure (`cli_run.rs:85-95`), and no `stdout.contains("SKIP")` shortcut may bypass
  that assert (CIA-4). Where CI can cheaply provide the prerequisite (CH-5 runtime), it does, so the test measures something.
- Fix the test's hermeticity (CH-3) rather than the runner's config, because a global identity in CI would hide the next such test.
- Host-scoped flags (CH-1) mirror `gate.sh:138`, so CI and local read the same rustflags sources.
- A reserved exit code is never a test witness (CH-4); witnesses are transcript lines and the program exits 0.
- Triage of unmasked failures follows the CH-2 rule and caps (CID-3); the builder never adds `#[ignore]`.
- CI changes are verified on a draft PR (plus `workflow_dispatch`), never by pushing to `main` (CID-2).
- Network installs in CI retry the whole block under `pipefail` and log each failed attempt (CIA-5).

## 9. Needs human

**NH-1. Require CI on ruleset 26386102** (today: `non_fast_forward` and `deletion` only, enforcement active). Required status
checks also bind **direct pushes**: "after all required status checks pass, any commits must either be pushed to another branch
and then merged or pushed directly" (GitHub docs, *About protected branches*). The integrator pushes merges straight to `main`.
With the check required, each push would need its commit to have already passed CI on another ref. `ci.yml` triggers only on
`main` pushes and PRs (plus `workflow_dispatch` after CH-2), so merges would go through a pre-verified branch or PR, and every
merge would add the CI wall time (5-7 min today, more after CH-2). **Recommendation: not yet.** Require the three Linux jobs plus
`Docs & spec hygiene`, never iOS (it is paths-filtered, `ios.yml:24-31`, so a required iOS check would block every unrelated
push), and only after **N = 10 consecutive green push runs on `main`**. `[REVISED: CID-6]` Why 10: not a flake statistic. One
DNS failure in 303 logged runs is a single event (95 % interval ~0.3-53 % `[INFERENCE: Clopper-Pearson]`), and green runs never
exercise the retry. N = 10 is a stability window: at the observed cadence (24 runs in 2026-10-26..11-04, ~2.7/day) it is ~4 days
of ordinary merges with no red, enough for the CH-2-unmasked suites to meet a range of unrelated changes. A run that fails only at
`Install LLVM 23` after 3 logged attempts does not reset the count, and is logged. Any other red resets it. CH-7 itself is judged
by a logged `apt attempt k failed` followed by success, not by the streak.

**NH-2. iOS job scope.** A: author `run_on_sim.sh` and `Package.swift` (needs a macOS host, which cannot be verified from here).
**B (recommended): drop the step and rename the job to what it verifies (CH-6), and log the remainder.** C: keep it as it is
(always red).

**NH-3 (`[REVISED: CID-3]`). Real bugs CH-2 unmasks outside Lane C.** A: mark each `#[ignore = "O0NN: <class>"]` until its owner
fixes it (CI goes green; that test then measures nothing). **B (recommended): leave the test failing, file the O-entry, and hold
the NH-1 count until it is fixed**, since an ignored test is a skip the "a failure is never a skip" rule forbids. C: per-test
choice by the integrator.

`[DECIDED by human 2026-11-05: NH-1 not yet - require the three Linux jobs plus Docs & spec hygiene (never iOS) only after 10
consecutive green push runs on main; NH-2 B - drop the simulator step and rename the job; NH-3 B - leave unmasked out-of-lane
failures failing, file the O-entry, hold the NH-1 count. Ruleset 26386102 is not changed by this batch.]`

## 10. Verification log (all at 9300d9fd, 2026-11-04)

- `gh api …/workflows/ci.yml/runs?per_page=100&page=1..4` → 303 runs, all `failure`. `--status success` → `[]`.
- `gh run view 41875787424 --log-failed` and seven earlier runs: the same 5 failures since ebbb4e02. 8b1c83b1 hit the apt DNS flake.
- `run…log:1526-1527`: the REDTEAM line, followed by `goal_demos_pure_outcomes ... ok`.
- Workspace job (`RUSTFLAGS=""`): both wasm tests `ok` (`:2868`, `:3124`).
- git identity probe (CH-3): exit 128 without identity, 0 with `-c`.
- `lifecycle.ax` direct run: `main returned 3 … Exiting 1`, exit=1. The harness without an NDK prints `SKIP — Android NDK not found`.
- `cargo fmt --all -- --check` → exit 0, no output, 2.6 s (O005).
- O007 and O012 anchors read as cited in CH-9. `gh repo view` → PUBLIC. Ruleset 26386102 has rules `non_fast_forward` and `deletion`.
- iOS: 16/16 failures at the same step. `tools/ios-parity-harness/` = `README.md` only. Reference `axon run
  examples/mobile/ios_window_clear.ax` → `rendered 1 frame(s)`, exit 1.
- Step 1 re-checks: `main.rs` prerequisite/link-failed message lines (CIA-1); `v022_g01_mutations.py:8820-8828` drift refusal and
  `v022_attack_markers.py:4165` format (CIA-2); `android_lifecycle.sh:13,81,90,92`, `R14-mobile-targets.md:546` (CIA-3);
  `cli_run.rs:1004` and `:85-95` (CIA-4); `ci.yml:200-209` (CIA-5); `Cache not found` at `run…log:143,3412,6510` (CID-1).
- Not verified: anything needing a codegen binary, a cargo build, a macOS runner, or a booted guest (marked `[INFERENCE]` above).
