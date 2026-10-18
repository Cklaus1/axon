# S1: one governed repair episode (R0-R8)

Source: the user's go-ahead (2026-10-17) on "build one real, governed self-repair episode end to end, using the
parts that already exist". Base: `main` f83c6f86, branch `repair/int`, worktree `/home/cklaus/projects/axon-lanes/rep-int`.
Seams were mapped by two read-only scouts (`agent://SeamCortexRepair`, `agent://SeamAuthorityLoop`). Step 1 adversarial
review (reviewer agent, 28 findings, 2026-10-17, `agent://ReviewRepairSpec`) is folded in as `[REVISED: …]` markers; the
original text is the first commit of this file.

Why this slice: the user's ASI OS v0.3.0 pack (`build/FIRST_TASK.md`) and Cortex v0.23 (B291, B300) both name "one real
local test-repair path through existing owners" as the first build, with optimization record-only.

Acceptance proofs (the pack's, on real code): (P1) a good candidate passes; (P2) a bad candidate fails; (P3) a capability
the original code did not hold is blocked before the candidate executes; (P4) the accepted artifact's digest equals the
acceptance-evidence digest; (P5) budget or deadline exhaustion → no new generator calls, and no audit call starts after
the deadline `[REVISED: was "no new model calls", review #14 — the ranker runs before the first proposal]`.

Shared acceptance for every item: `scripts/gate.sh` shows no new failure against the baseline;
`python3 -B scripts/v022_refusal_coverage.py` and `scripts/v022_g01_mutations.py` stay green, every new refusal site
carrying a killed mutation row or a reasoned exemption and every moved anchor re-pinned (M55, M71, M81, M84 anchor
`run_checks` text in `runner.rs`; 18 rows anchor in `axon-cortex/src`) `[REVISED: review #26]`; `cargo fmt --all --check`;
`python3 -B scripts/pci_delta.py --check` (pci theme per `crates/*/src` commit); `reference_gate.sh` when a verb or env var
is added; `AXON-COMPLETENESS.json` rows corrected and `.md` regenerated (integrator, last); `CHANGELOG.md` (integrator).
Test fixtures pass ABSOLUTE `--workspace` paths (relative is broken, pre-existing, review #27 → opportunity).

## Defect that makes R1 P0 (reproduced 2026-10-17 at f83c6f86, and again by the reviewer)

`cortex repair --workspace W --file broken.ax --symbol double --check hidden_completion --write-prefix broken.ax
--candidates 1 --budget 4 --generator 'literal:\n    let _ = write_file("/var/tmp/r51-tmp/PWNED", "…")\n    n * 2\n'`
→ exit 0 `verified_done`, the patch is KEPT, and `/var/tmp/r51-tmp/PWNED` exists. A model-proposed candidate runs with the
operator's full host authority during its checks. `registered_runner` (`axon-cortex/src/bin/cortex.rs:295-330`) sets none
of `with_effect_ceiling`, `with_timeout`, `with_max_output`, `with_clean_env`.

Why the obvious fix fails `[REVISED: review #1, #2, #4 — the first draft's R1 was wrong]`: `axon_os::runtime::scan_effects`
is a hand-written textual list that misses `append_file`, `file_copy`, `file_rename`, `dir_create`, `dstore_apply`,
`sandbox_run`, ignores println/Random/Time, and flags `mod` inside comments. The runtime ceiling `AXON_ALLOWED_EFFECTS` has
one `IO` bucket covering both `println` and `write_file`: an empty ceiling fails pristine checks in 10/115 receipt tasks and
9/47 heldout tasks (println, eprintln, `gaussian_sample`), while `IO` re-admits `append_file` (reviewer repro: exit 0, file
written). 52 of 55 receipt sources use println.

## R0 — Interpreter primitives the fence needs (P0, axon-core; prerequisite of R1) `[REVISED: new item, review #1, #2]`

- **R0a — `axon caps <file> [--fn NAME] [--json]`.** Reports the capabilities a program (or one fn, transitively) can
  exercise, computed by the SAME walker `@[contained]` uses (`capabilities.rs`: `classify_call_paths`, helper-follow,
  method dispatch, `check_builtin_value_ref` for builtins used as values), so the static fence and the language's own
  containment can never disagree. Output `{"schema":"axon-caps/1","file","fn","capabilities":[…]}` where capabilities
  are the `IoKind` labels (fs_read, fs_write, net, exec, env, durable_store, ai, …: whatever `IoKind` defines) plus
  `unknown` when the walker meets something it cannot classify (deny by default). Reads `-` as stdin so a caller can
  check text it has not written. Parse/type errors → exit 2 with the diagnostics (cortex treats that as DidNotCompile).
  Follows imports through the shared loader (closure capabilities count).
- **R0b — ambient path scope.** `AXON_ALLOWED_FS_READ`, `AXON_ALLOWED_FS_WRITE`, `AXON_ALLOWED_NET` (comma-separated
  prefixes / hosts, same syntax as `sandbox_create_scoped`) populate the ambient `SandboxScope` (`interp.rs:1558`,
  `ambient_sandbox` at 7487) that is `SandboxScope::default()` today. Unset = unrestricted (today); empty = deny all.
  Independent of `AXON_ALLOWED_EFFECTS` (an unset effects ceiling plus a set scope restricts only paths/hosts). A true
  ceiling an inner sandbox may narrow, never widen. `env_registry.rs` rows, `AXON_REFERENCE.md` regenerated, control
  rows with every engine state, native: refused at build time and by an `axon-rt` REFUSED row (house rule), added to
  `scripts/build_outcome_regression.sh`.
- Tests (fail-first): `axon caps` on a fn that reaches `append_file` through a helper and through a builtin alias reports
  `fs_write`; on a pure fn reports `[]`; agreement test: for every `examples/**/*.ax` fn carrying `@[contained]`, a
  capability `axon caps` reports outside the spec ⇔ `axon check` E1001. `AXON_ALLOWED_FS_WRITE=` with effects unset:
  `println` works, `write_file`/`append_file`/`file_copy` dest/`dir_create` refused; a prefix admits only under it;
  `AXON_ALLOWED_FS_READ=` refuses `read_file`/`file_exists`.

## R1 — Candidates cannot acquire a capability the original code did not hold (P0, security; critical path)

`[REVISED: rebuilt on R0, review #1-#7]`
- **R1a — the grant.** Before the walk, for each repair target fn, cortex runs `axon caps <file> --fn <target> --json` on
  the PRISTINE file: the target's transitive capability set is the static grant (invariant I-12: a repair may not
  acquire a capability). The runtime fence is derived from `axon caps <file>` (whole pristine file): the free set
  {IO-console, Random, Time, Chan, Pure} is always granted; `Exec`/`Net`/`AI` enter `AXON_ALLOWED_EFFECTS` only if the
  pristine file uses them; `AXON_ALLOWED_FS_READ`/`FS_WRITE`/`NET` are empty unless the pristine file uses that kind (then
  unset for that kind: path narrowing is out of scope). Recorded in the episode and `cortex-repair/1` as
  `grant:{capabilities:[…], basis:"axon-caps"|"unknown", fence_env:{NAME:value}, fence_sha256}` — no fabricated
  `axon_os::Grant` fields `[REVISED: review #7]`. `unknown` in the pristine set → no static fence for that target, runtime
  fence unset for the unknown kind, `basis:"unknown"`, one stderr line `[REVISED: review #4]`.
- **R1b — admission before the candidate executes.** Insertion point: after `validate`, before `execute` writes
  (`runner.rs` ~1062-1130) `[REVISED: review #5]`. Cortex runs `axon caps - --fn <target>` on the in-memory patched text
  (nothing written, nothing to revert). Any capability not in the static grant → a NON-terminal rejected attempt
  (`push_rejected` + `AttemptSignal`), recorded as the existing `ActionDenied{action, reason:"capability:<kinds>"}`
  event, fed back to the generator as `refusal:{capabilities}`; it counts against `--max-proposals`; in early-stop it
  is skipped like `DidNotCompile` (`earlystop.rs:111-121`) `[REVISED: review #6]`. No check of that candidate runs.
- **R1c — runtime fence.** Every spawn that executes workspace code (visible run, hidden check, `check_outcomes`, the
  pristine pre-flight) carries the fence env and `with_clean_env` (reviewer: `env -i` over 74 corpus sources, no
  difference). `axon check` spawns need no fence (it does not execute code; `comptime` write tested inert, review #11).
  An empty value stays empty (M55's mutation — empty becomes unset — must be killed by a cortex-side test).
- **R1d — pre-flight fence conflict.** The existing pristine pre-flight runs under the fence; any test failing with a
  sandbox/ceiling violation → exit 22 `fence_conflict` naming the test and the kind. Never silently weaker.
- **R1e — `cortex grant --workspace W --file F [--symbol S] --json`** prints the derived grant and fence env without
  running anything (operator visibility; the offline sweep's input).
- **R1f — offline fence sweep (hard gate before any paid run, in `gate.sh`).** `crates/axon-cortex/benchmarks/
  fence_sweep.py`: for every task in the receipt corpus and heldout1, `cortex grant`, then `axon test --json` on the
  pristine (and pristine-repaired, where the corpus has it) source with and without the fence; zero status flips
  required. ~2-10 s, no USD.
- Tests (fail-first): the PWNED row in `tests/cli.rs` (today exit 0 + file exists; after: no file, `action_denied`
  reason `capability:fs_write`, exit ≠ 0); `append_file` via an existing helper in a file whose other code legitimately
  prints (the reviewer's bypass); a builtin alias (`let f = write_file`); a correct candidate on the same fixture still
  exits 0 (P1); `fence_conflict` row; runtime-fence row: a fixture whose static caps say pure but whose test needs a
  denied kind at runtime shows the violation (or, if none exists, the env reaches the check and an M55-style mutation
  is killed). Fabric executor path: regression only.
- `--effect-ceiling` dropped from S1 `[REVISED: review #3 — narrowing below the pristine set denies every candidate;
  needs its own design → opportunity]`. No new Cargo dependency: cortex calls the `axon` binary it already pins.

## R2 — Every execution is bounded and killed as a group (P0, containment)

- `registered_runner` sets `with_timeout` (new `--check-timeout-ms`, default 60000), `with_max_output` (1 MiB),
  `with_clean_env` (with R1c).
- `run_limited` (`runner.rs:3018-3115`) spawns through `axon_jobctl::Job` (own process group, `killpg` at the deadline,
  bounded drains), replacing `child.kill()` on the direct child and the unbounded drain joins.
- `axon check` spawns in `observe` (~457), `checker_exit_clean` (~1704) and `visible_run`'s compile (1779-1782) get a
  timeout and a `Job`, not the fence `[REVISED: review #11]`.
- A timed-out or truncated check of a CANDIDATE state is a rejected attempt + revert, never exit 22, at all three sites:
  RunCheck→`Failed`→`Blocked` (`runner.rs:655-661, 1210-1213`), `check_outcomes` after verify (1361-1372),
  `checker_exit_clean` (1390-1415) `[REVISED: review #8]`. `timed_out` is fed back only for VISIBLE runs; a hidden-check
  timeout is a plain rejection (no bit about the grader leaks) `[REVISED: review #9]`. A timed-out PRISTINE pre-flight
  stays exit 22. Skipped in early-stop like `DidNotCompile`.
- Tests (fail-first) use `--check-timeout-ms 1500` and assert completion within timeout + 2 s drain grace
  `[REVISED: review #10]`: a candidate body that never returns → today the CLI hangs (outer guard fires); after: rejected,
  next proposal runs. Group kill: a fixture whose pristine uses `exec`, so R1 admits a body that backgrounds a grandchild
  holding the pipes; after the timeout no grandchild survives and the loop is not stalled.

## R3 — Walk budget and deadline: exhaustion means no new generator calls (P0, P5)

`[REVISED: review #12-#15]`
- New `--max-proposals N` (total generator calls across the whole walk, one counter shared across pass 1 and pass 2,
  enforced in the walk driver, NOT via `CandidateProgress.proposal_limit` which drives the pass-1 pause) and
  `--deadline-ms N` (wall clock from cortex start, so the ranker counts). Both default to unset (unlimited), so the
  PREREG-regress configuration is unchanged.
- On exhaustion: no further `gen.propose`. `cmd:` generator deadline = min(remaining, its 120 s timeout) via its existing
  `Job`; an `ai:` call is not started when the remaining time is below its HTTP timeout (an in-flight `ai:` call cannot be
  cancelled — documented). The audit is not started after the deadline. A check already in flight at the deadline
  completes (bounded by `--check-timeout-ms`), and a pass it produces is accepted (it needs no model call).
- Distinct internal outcome (`WalkBudgetExhausted{by}`), mapped to exit 20 `budget_exhausted` with `detail` =
  `proposals` | `deadline`; existing step exhaustion keeps `detail` empty. `cortex-repair/1` gains
  `budget:{max_proposals, deadline_ms, proposals_used, elapsed_ms, exhausted_by}`.
- Tests (fail-first): logging `cmd:` generator that always answers wrong: `--max-proposals 2 --candidates 3` → exactly 2
  lines in `calls.log`, exit 20 `proposals`; slow generator + `--deadline-ms` → exit 20 `deadline`, no call starts after
  it, the in-flight generator's group is gone; R1-refused proposals count.

## R4 — Diagnostics reach the generator as structured data (P1, capability)

- `RejectedAttempt.feedback: Option<String>` (`generate.rs:144-153`) becomes a typed `Feedback { diagnostics:
  Vec<Diag{file,line,col,code,severity,message,help,expected,found}>, failed_tests: Vec<{name,message}>, refusal:
  Option<{capabilities}>, timed_out: bool, note: Option<String>, truncated: u32 }`, parsed from `axon check --json` /
  `axon test --json` (`axon-diag/1`, `axon-core/src/lib.rs:282-345`) instead of `render_diagnostics` prose
  (`runner.rs:2165`). `file` is rewritten from the visible copy's `.cortex-visible-<pid>-<name>` to the target's relative
  path (no pid in prompts) `[REVISED: review #16]`.
- `build_prompt` (`generate.rs:264-361`) renders one compact JSON block per rejected attempt; per-field caps
  (message/help 300 B, ≤8 diagnostics, ≤8 failed tests) with an explicit `truncated` count replace the 2 KB blob cap.
- Grader secrecy unchanged: `cxg_c14_a_rejected_attempt_is_fed_back_without_leaking_the_grader` and `tests/cli.rs:1357`
  stay green, plus a row asserting no hidden-check name/text in the JSON block. Fabric executor's empty `messages`
  (`runner.rs:3531`) unchanged (opportunity).
- Tests (fail-first): a `cmd:` generator copies its stdin to a file; a first wrong candidate that type-errors → the second
  prompt contains a JSON block with the diagnostic's `col`, `expected`, `found` and the relative `file` — red today.

## R5 — Accepted artifact digest equals the acceptance evidence (P1, P4)

- `[REVISED: review #19]` The DECIDING `Verified` event (verify's `passed:true` on exit 0; on exit 27 the adjudicated
  event) gains `graded_sha256`: sha256 of the workspace file taken before the hidden check spawns and re-taken after the
  LAST acceptance spawn (`check_outcomes`, `checker_exit_clean`); a difference fails closed (`passed:false`, "workspace
  changed during acceptance").
- `cortex-repair/1` gains `accepted:{path, sha256}` on exit 0 and 27 (patch kept), `null` otherwise.
- Invariant, tested: exit 0 ⇒ `accepted.sha256 == graded_sha256 == sha256(file on disk after exit)`; extend
  `cli_ships_evidence_a_reader_can_verify` (`tests/cli.rs:1848`).
- `run_ab.py` `[REVISED: review #17, #18]`: in `run_arm_b` only (`grade()` is shared with arm A and stays), compute the full
  binary sha256 of the final file and compare with `accepted.sha256` when present; a mismatch ABORTS the run as an
  integrity failure (not scored), so incumbent rows without `accepted` cannot be favoured.
- Tests (fail-first): field absent today → red; a `CheckExecutor` double that edits the file during acceptance →
  `passed:false`.

## R6 — Record-only trial ledger in axon-loop — `[REVISED: DEFERRED → opportunities, review #20]`

The ledger would be self-signed, nothing in S1 reads it, and reusing `promote.py`'s `Evidence.trial` changes the meaning
of `ws_out` (`promote.py:399`, the row digest the promotion pipeline uses). Revisit with NH-R1 (an envelope token gives
the ledger an independent signer) and when a learner exists to read it.

## R7 — Evaluation (P1)

- **R7a regression (paid, ~$0.15, ~6 min).** `scorecard.py regress` on the receipt corpus with the frozen S1 source. Rule
  `[REVISED: review #21 — quoted, not paraphrased]`: PREREG-regress amendment 3 — any false success fails; then McNemar
  exact p<0.05 on the pooled 115 tasks; then sign tests on proposals, wall time and cost per task. "108/115" is main's
  measured score, not a threshold. Additional S1 rule `[REVISED: review #22 — R7a cannot see a 4-task loss]`: zero tasks
  whose checks print "not in the active sandbox's allowed set" / "sandbox violation", and R1f green before the paid run.
- **Sequencing** `[REVISED: review #23]`: R8's USAGE text lands with the code; freeze source → commit → `regress` → commit
  its history row → full `gate.sh` (whose `scorecard.py receipt --check` fails until the row exists). Any later edit to a
  file in the repair digest (`axon-cortex/src/*.rs`, `bin/*.rs` except `ai.rs`/`label.rs`, `benchmarks/drivers/*.sh`)
  requires another paid run, so fmt/clippy/comment fixes go in before the freeze.
- **R7b fresh held-out — `[REVISED: DEFERRED → opportunities, review #24, #25]`.** Every existing set is saturated at
  f83c6f86 (gencontext: heldout 47/47, once 30/30); only 16 unused files exist; at n≈50 a PASS needs ≥6:0 discordant
  pairs, so S1 cannot show a capability gain and a fresh set spent on non-inferiority duplicates R7a. Next-run candidate:
  build a held-out set with headroom, then measure R4.
- `hard-once` is NOT spent (NH-R2).

## R8 — Docs and tracker (P2)

USAGE text in `bin/cortex.rs` and `crates/axon-cortex/README.md`: grant, fence, timeouts, walk budget, `accepted`,
`cortex grant`; `axon caps` and the scope env vars in `spec/stdlib.md`/CLI docs as the house does; CHANGELOG; tracker rows
(`cortex repair` loop, repair containment, `AXON_ALLOWED_FS_*`/`NET` controls); opportunities.

## Cost budgets (dev-loop review)

| Check | One run | Scarce resource | Re-run trigger | Skip rule |
|---|---|---|---|---|
| `cargo test -p axon-cortex` | ~60 s test, 2-5 min with build | CPU, cargo lock | cortex/axon-core/jobctl change | inputs unchanged |
| axon-core lane tests (`cli_run` slice + new caps/scope tests) | 5-40 min (`cli_run` alone 2289 s) | CPU | axon-core change | run filtered tests in-loop; full at gate |
| `gate.sh` | ~36 min, ~153 CPU-min | host CPU, exclusive worktree | final head | per wave + final only |
| g01 mutations | 1-3 min/row | worktree mutated in place | anchors moved | `--changed-since` in lanes; full scope once at the end |
| R1f fence sweep | 2-10 s | none | caps/fence mapping change | every such change |
| R7a paid regress | ~$0.15, ~6 min | USD (cap $1/invocation, run budget $3), micode rate limit | repair-digest change | keyed on digest |

## needs-human (excluded from the autonomous run)

- **NH-R1 episode approval token.** For unattended episodes an operator would approve an envelope (task, write prefixes,
  grant, check digest, budget) with a keyed token a supervisor verifies. Per-candidate `axon-approval/3` tokens are the
  wrong shape: they bind entry bytes, so each patch needs a fresh approval, and an automatic one is the machine
  approving itself. Recommendation: build after S1 as an `axon-approval` sibling schema reusing the G5 key derivation.
  Until then the operator's command line is the authority, and deploying the accepted artifact uses the existing human
  `axon ast approve` / `axon-intent approve`.
- **NH-R2 spend `hard-once` (n=19) on S1.** Scored once per candidate; the final judge. Recommendation: keep it for a
  candidate that has first shown a gain on a held-out set with headroom.

## Out of scope / open

Learned repair policy; Forge SIR / AIR / IntentIR types; keyed sealing of the cortex episode (`axrmac1`); USD metering
inside the loop for `cmd:` generators (harness `--cap-usd` stays); Fabric executor feedback; a richer edit vocabulary than
whole-body replacement; path-level narrowing of a capability the original already holds; `--effect-ceiling`; relative
`--workspace`; R6; R7b.
