# Follow-up spec: S1 follow-ups (held-out headroom set, capability walkers, cortex fixes)

Status: DRAFT 2026-10-18. Branch `fu/int` (worktree `/home/cklaus/projects/axon-lanes/fu-int`), base `main` f26e9506.
Source: S1 end-of-run opportunities (`tasks/opportunities.md` O-S1-02, 04, 09, 10, 11, 12, 13, 14, 16, 18) and the
2026-10-18 human decisions (`tasks/repair-spec.md` NH-R1/NH-R2 DECIDED; `tasks/approval-residue-spec.md` H4).
Evidence maps: `local://heldout-map.md`, `local://walkers-bugs-map.md` (code anchors below come from them and were
re-read where quoted).

Out of scope (separate DRAFT specs, written after this batch ships): NH-R1 unattended authorization and its
enforcement gate (`tasks/unattended-auth-spec.md`), R6 trial ledger. **No unattended execution is authorized by this
spec.** Settled decisions NH1/H1, H3, KG4, H4, Fabric digest channel H2 are not reopened.

## Priority tiers

| Tier | Items |
|---|---|
| P1 | FU-1 headroom held-out evaluation (O-S1-02), FU-2 capability walkers (O-S1-14) |
| P2 | FU-3 drain stop state (O-S1-12), FU-4 restore only our own bytes (O-S1-13), FU-5 non-UTF-8 effect ceiling (O-S1-09 narrow) |
| P3 | FU-6 relative `--workspace` (O-S1-04), FU-7 lexer-aware `fn_body` (O-S1-11), FU-8 `run_ab` integrity-abort fixture (O-S1-10), FU-9 build-loop skill lessons (O-S1-16) |
| R4 | FU-10 R4 keep/remove criterion (O-S1-18): fixed here, before any measurement |
| eval | FU-11 evaluate the batch candidate on the frozen held-out set (after merge) |

Repair digest (`crates/axon-cortex/src/*.rs` + `src/bin/*.rs` minus `ai.rs`/`label.rs`, plus `benchmarks/drivers/*.sh`)
changes in FU-3, FU-4, FU-6, FU-7 → one paid receipt `regress` on the integrated head before merge (no waiver).

---

## FU-1 (P1) Headroom held-out evaluation — O-S1-02

**Problem.** Every existing set is saturated for the incumbent generator (`claude-haiku-5-5` via micode): dev 60/60,
heldout/once at ceiling, receipt 107/115 against a ceiling of 108, hard-dev 24/25 in scope. A same-code replicate gave
0/115 discordant pairs, and a McNemar gain needs ≥ 6:0 discordant pairs, so no candidate can show an improvement.
Weaker generators are unreachable (claude-3-haiku and claude-3.5-haiku give 404, OpenRouter via micode is refused,
llama/qwen give 401: `B/scorecard/PREREG-regress.md:162-171`). So headroom has to come from harder tasks, not a weaker
model.

**Goal.** A new task set `hx` with a dev split for development and calibration and a frozen test split. On test, the
incumbent should solve roughly 20–80% of tasks. The set must be powered to detect a meaningful candidate gain, and it
must be protected by digest pins and a spent ledger.

### FU-1a Task classes (mechanically derived; every task single-symbol in scope)

Cortex replaces one function body per attempt (`earlystop.rs:1-29`). Every `hx` task is therefore built so that
restoring one function body fixes it. `B/hardset/build.py:155-168` already proves the reverse check for out-of-scope
tasks; hx reuses that check and asserts it the other way.

| Class | Construction | Build-time validity checks |
|---|---|---|
| `multi` | 2–3 independent mutations in ONE function body | broken file passes `axon check`; each mutation alone fails ≥1 hidden test; restoring the body passes all |
| `caller` | mutate callee `g`; visible failing checks call only a caller `f` (g's own visible tests withheld; hidden keeps them) | the visible failure does not name `g`; `g` is reached by every failing visible check (`locate::reached_by_all`), so the task stays in cortex's reach |
| `subtle` | new type-correct operators: loop/range bound ±1, swap of two same-typed call arguments, read of the wrong same-typed struct field, accumulator init swap, dropped `else` arm value | `axon check` clean; ≥1 hidden test fails; no visible test fails by a type or parse error |
| `refine` | programs using refinement `where` / struct `where` / `@[verify]`: the mutation surfaces as a refinement or verify failure, not an assertion | failure text names the refinement/verify; restoring the body passes |
| `caps` | the program contains a `@[contained]` or capability-narrow target plus a workspace data file holding the expected outputs (a tempting route via `read_file`) | pristine `axon caps` of the target has no `fs:read`; grading under FU-1d flags any widening |

Mutation operators live next to the existing ones (`B/heldout/mutators.py`, `B/common.py`). The builder is a new
`B/headroom/build.py` that writes `corpus.load`-format tasks (`B/ab/corpus.py:278-293`) and supports `--check`
(deterministic rebuild equals the committed file).

### FU-1b Sources and contamination control

- **Stratum D (derived):** repo programs that no existing corpus uses: the 17 hardset snapshots without tasks, plus
  `traits_demo` (reconstruction in `heldout-map.md`). They are public in the repo, so contamination is possible.
- **Stratum C (control):** 12–16 new programs written independently for this set. The authoring agent works only from
  short neutral problem statements and never sees existing benchmark sources, cortex prompts or results. Tests are
  committed before any defect is derived (anti-fitting order). The programs live in `B/headroom/programs/`.
- **Contamination check:** report solve rate per stratum on test. If stratum D's incumbent solve rate exceeds C's at
  one-sided p < 0.05 (cluster permutation, below), the result is flagged `contamination-suspect` and the gain is
  reported on stratum C alone.

### FU-1c Split, calibration, freeze

1. Build the pool. Split it into dev and test **by source program** (all tasks of a program go to one split), using a
   seeded shuffle with the seed recorded in `B/headroom/PREREG.md` **before any model run**.
2. Calibrate on **dev only**:
   - Incumbent = `main` f26e9506 cortex, arm B, `claude-haiku-5-5`, k = 3 trials per task.
   - The per-(class, knob) cell solve rate decides the recipe: keep the cells whose dev rate is in [0.2, 0.8].
   - Dev tasks are kept individually by their own rate in [0.2, 0.8] and form `hx-dev` for future development.
3. Apply the recipe to test **by cell membership only**. No test task is run or selected by its outcome. If the kept
   test pool is smaller than the power analysis needs, build more pool from the same recipe (new programs or mutation
   seeds) and split it again by program before any run.
4. Freeze the test split:
   - commit `B/headroom/hx-test.json`;
   - pin its sha256 in `PROTECTED_SETS` (FU-1e);
   - after the freeze, `PREREG.md` gets only appended result rows; its design sections are not edited.

### FU-1d Grading additions (hx only)

- `run_ab.py grade` runs `axon caps --json` on the given (broken) file and on the final file. It records
  `caps_widened` (new kinds) in every row.
- For tasks whose meta carries `caps_strict: true` (all hx tasks), `verified_clean` also requires
  `caps_widened == []`.
- Existing sets keep their scoring, so receipt comparability against pinned incumbent rows is unchanged.

### FU-1e Integrity: pins, spent ledger, hard-once

- `PROTECTED_SETS` (new, in the existing `B/scorecard/scorecard.py` `ONCE_SETS` area) maps set name → corpus sha256 for
  `hx-test` and `hard-once` (`3ebb8d12…ff8c8d6`, currently pinned by no gate).
- `run_ab.py` (not only `scorecard add`) refuses a corpus whose sha matches a protected pin unless `--spend PURPOSE` is
  given. Before any model call it appends a row to `B/heldout-spent.jsonl`:
  `{ts, set, corpus_sha256, purpose, repair_digest, generator, model, k, git_head, prev_row_sha256}`.
  The rows are hash-chained.
- A second spend with the same `(set, repair_digest, model, purpose)` is refused. This stops re-rolling a result. A
  rerun after an infrastructure failure needs `--rerun-of ROW_SHA` and the failure reason. The rerun row is recorded
  and both runs are reported.
- `scripts/gate.sh` gets one cheap stage:
  - verifies each protected corpus file's sha against its pin;
  - verifies the ledger chain;
  - runs `B/headroom/build.py --check` (deterministic rebuild);
  - and, since they are cheap and missing today, `B/heldout/build.py --check` and `B/hardset/build.py --check`.
- **hard-once is not spent in this batch.** The batch's acceptance requires the ledger to have no hard-once row.
- Honest limit: the ledger records spending and stops accidental re-use. It is not a security boundary, because a
  copied corpus file bypasses it. This is stated in `B/headroom/README.md`.

### FU-1f Power analysis (sizes the test split; no fixed 80)

- Statistic: per-task difference of success rates over k trials, candidate minus baseline (paired, same tasks). The
  test is a one-sided sign-flip permutation test **clustered by source program** (10 000 permutations), α = 0.05. The
  CI is a 90% cluster bootstrap.
- Power is simulated by `B/headroom/power.py`:
  - per-task incumbent rates come from the dev calibration (kept cells);
  - effect model: a uniform logit shift giving a mean +10 points (the MDE);
  - between-trial noise is Bernoulli at the per-task rate;
  - clustering uses the dev programs' tasks-per-program distribution.
- Choose the smallest (n_test, k) with k ∈ {3, 5} reaching power ≥ 0.80, subject to the FU-1 cost cap. The script's
  output and seed are committed in `PREREG.md` before the freeze.
- [INFERENCE, to be replaced by `power.py` output] Back-of-envelope: at p ≈ 0.5, k = 3, ignoring clustering,
  sd(d_i) ≈ 0.41, so about 150 tasks give power ≈ 0.85 at MDE 10.

### FU-1g Repeated trials and second generator model

- Model variance: every arm runs k trials per task with the same config (`run_ab` gets `--trials K`; rows carry
  `trial`). The report gives per-task between-trial variance and the same-code replicate discordance.
- Second generator for confirmation: `claude-haiku-4-5-20251001` via micode. It is reachable, priced at 1/5 per MTok in
  `run_ab.py:79-83`, and a different model generation. Sonnet 4.5 is reachable too but is stronger and likely
  saturates the set. Non-Anthropic generators have no driver (OpenRouter serves only the ranker) and are out of scope.
- The ranker stays `openrouter:anthropic/claude-haiku-5.5` for both generator models (already independent:
  `CORTEX_MODEL` vs `CORTEX_RANKER`).

### FU-1h Gain criterion (pre-registered; used by NH-R2's hard-once spending gate)

A candidate shows **meaningful task-solving improvement** on `hx-test` when ALL hold:
- the primary model (`claude-haiku-5-5`) gives one-sided cluster permutation p < 0.05 and Δ̂ ≥ +5 points;
- false successes ≤ the baseline's;
- the second model gives Δ̂ > 0.

Meeting this criterion makes a candidate *eligible* to spend hard-once. Spending still needs explicit human approval
(NH-R2 = A).

### FU-1 cost and acceptance

- Paid budget: dev calibration ≤ $3, test evaluation (FU-11) ≤ $25 in total, each run under `--cap-usd`.
  Costs observed so far: ≈ $0.0012/task/arm for haiku-5.5; ≈ 5× that for haiku-4.5 on the dev leg.
- Acceptance:
  - `hx-dev` and `hx-test` are committed and pinned;
  - `PREREG.md` holds the seed, recipe, power output and criteria, committed before the freeze;
  - the incumbent's dev-calibrated rate is in [0.2, 0.8];
  - the gate stage passes;
  - the ledger has no hard-once row;
  - fail-first tests (pytest-style self-tests next to the scripts, run by `gate.sh`) cover:
    - the refusal of an unspent protected corpus;
    - duplicate-spend refusal;
    - chain-tamper detection;
    - `caps_strict` grading (a fake final file that adds `read_file` is not `verified_clean`).

---

## FU-2 (P1) Capability walkers see predicate routes — O-S1-14

**Problem.** `program_capabilities` (`crates/axon-core/src/capabilities.rs:903-923`) and `program_builtin_calls` /
`program_effect_rows` (855-901) root only `FnDef.body`, impl-method bodies and `LetDef.value`.
`check_native_grants` walks `fndef.body` only (502). A builtin or native call written directly in any of these is
invisible:
- a `RefineDef.predicate` (named, and the synthetic `__refine_*` from inline `where`);
- a `TypeDef.refinement`;
- a `FnDef.verify.predicate` (free fns and impl methods).

E1209 rejects impure builtins in refinement predicates of type-checked programs, but `@[verify]` has no purity check.
G2 and E1203 also see un-checked ASTs.

Affected consumers (`walkers-bugs-map.md` A.4):
- G2 E1402 (`improve.rs:164-178`)
- the E1203 import edge (`capabilities.rs:1173-1197`)
- `axon lock` and the E1204 reason (`audit.rs:100-130`)
- the G1 corpus purity filter (`main.rs:2015`)
- deploy risk (`main.rs:9123-9180`)
- `--emit-manifest` `effect_union` (`main.rs:8986-9006`) → axon-vm admit
- E1004 native grants

S1's `reachable_capabilities` (`axon caps`, `@[contained]`) already sees these routes.

**Change (narrow first).**
- Add one private item-roots iterator in `capabilities.rs` that yields every root expression:
  - fn bodies and their verify predicates;
  - impl-method bodies and their verify predicates;
  - `LetDef` values;
  - `RefineDef` predicates;
  - `TypeDef` refinements.
- Use it in `program_capabilities`, `program_builtin_calls` and `check_native_grants`. Walking every refinement
  unconditionally over-approximates, which is safe for these consumers because they need ⊇.
- Consolidating the walkers with `reachable_capabilities` happens only if the narrow change shows substantial duplicated
  traversal logic, i.e. more than the roots list. Otherwise it is logged with numbers in `opportunities.md`.

**Shared fixtures (the agreement test).**
- One fixture table in `crates/axon-core/tests/` with these predicate-only cases:
  - `read_file`/`write_file` called directly inside `@[verify]`;
  - inside a struct `where`;
  - inside a named refine;
  - inside an inline param refinement (un-type-checked parse where E1209 would otherwise intervene);
  - a native `M::f` call in `@[verify]`.
- Each case asserts that all four consumers report the same kind:
  1. the G2 diff: `improve` verify of a pass adding the predicate gives E1402;
  2. the import checker: a contained importer of the module gives E1203;
  3. the manifest emitter's `effect_union`, via `program_effect_rows` directly, plus a codegen-feature `--emit-manifest`
     test if the feature build is in the gate;
  4. S1's `reachable_capabilities`.
- One unit test per root kind goes next to `program_capabilities_sees_impl_method_bodies` (2620).
- g01 rows: one mutation per new root kind in `scripts/v022_g01_mutations.py`, in the same commit as the tests.
- Acceptance: the fixtures fail before the change on (1)–(3) and pass after; (4) passes before and after; the full
  axon-core suite has no new failures.
- Logged separately (not in scope): `@[verify]` has no purity check, while E1209 covers refinements only. That is a
  language-behavior change and goes to `opportunities.md` as O-S1-19.

---

## FU-3 (P2) A stopped drain never counts as complete output — O-S1-12

**Problem.**
- `drain_bounded` (`crates/axon-cortex/src/runner.rs:3742-3771`) leaves its read loop when `wait_readable` returns false
  (stop set), exactly as on EOF.
- `join_within` (`axon-jobctl/src/lib.rs:455-485`) sets stop after `DRAIN_GRACE` and returns `Some(partial)`.
- `joined_output` (3722-3730) then accepts a truncated capture as complete.
- The generator side (`generate.rs:524-549` `drain_capped`, joined at 660) has the same pattern.

**Change.**
1. Reproduce first:
   - an end-to-end test using a registered check wrapper (the `cortex-check-registry/1` pattern of
     `r2_a_timed_out_check_is_killed_as_a_group_and_the_loop_goes_on`, `tests/cli.rs:2439`);
   - the wrapper prints a passing result, then leaves a `setsid` grandchild holding the pipes past `DRAIN_GRACE`;
   - the test asserts that the check is NOT counted as passing;
   - plus a direct `drain_bounded` unit test (write end held open, stop set).
2. The drain returns an explicit stopped/incomplete state. `joined_output` maps it to `BoundExceeded::unread()`, so an
   incomplete check yields no verdict and can never pass.
3. `drain_capped` gets the same change: a stopped generator drain is an incomplete proposal and is rejected, not
   parsed. The pattern to follow is `axon-os/src/runtime.rs:312-366` `read_bounded`.
4. g01 rows for the stopped flag in both drains.

**Acceptance.** The reproduction fails before the change and passes after. Existing `r2_*`/`kg2_*` CLI tests are
unchanged and green.

## FU-4 (P2) Restore only bytes this run wrote — O-S1-13

**Problem.** `settle_artifact` (`crates/axon-cortex/src/bin/cortex.rs:1581-1594`) writes the pre-repair original back
on any `accepted_artifact` error. That includes "it changed after acceptance" (1566-1571), so it overwrites another
actor's edit. It also ignores a failed write (`let _ = std::fs::write`). The other restores at 1161-1163 and 1042 are
audited for the same pattern.

**Change.**
- The run tracks the sha256 of every byte string it wrote to the target: each candidate splice and the adjudicated
  bytes.
- Before any restore, read the file. Restore only if its sha is in that set. Otherwise leave the file as found and say
  so in the refusal reason (exit 22 stays).
- A failed restore write is reported in the reason and is not ignored.
- Rework:
  - rewrite `a_refused_settle_restores_the_file_as_found` (1800) to assert that external bytes survive, and that a
    non-`Changed` error (no graded digest, read-back error) still restores;
  - rework g01 M3714 so its kill test exercises the restore that remains;
  - re-verify the `original` strings of M3708/M3710/M3712/M3713/M3715/M3716.

**Acceptance.**
- The new test fails before the change: an external edit after acceptance is overwritten today.
- It passes after the change.
- A test with an unwritable target reports the restore failure.
- The r5_* CLI tests stay green.

## FU-5 (P2, narrow) Non-UTF-8 `AXON_ALLOWED_EFFECTS` denies every effect — O-S1-09

**Problem.** `interp.rs:7566` does `let Ok(raw) = std::env::var("AXON_ALLOWED_EFFECTS") else { … unset … }`.
`VarError::NotUnicode` is therefore treated as unset, which means every effect is allowed: a set ceiling fails open.
The scope variables already distinguish the two cases (`ambient_scope_var`, 7620).

**Change.**
- `NotUnicode` → an empty allowed set, which denies every effect, plus one stderr warning.
- The same fix applies at every other reader of the variable: `main.rs:6747`, and the native runtime if it reads the
  variable (axon-rt); the build enumerates the readers by grep and records the list in the commit.

**Acceptance.** A fail-first CLI test sets the variable to invalid UTF-8 bytes (`OsStr::from_bytes`). Today `axon run`
of a `println` program prints and exits 0. After the fix the run is refused with the sandbox-violation exit code.

The remaining O-S1-09 items (native-module scopes, `@[contained]` closures stored in module `let`s) belong to the
NH-R1 enforcement gate spec, not this batch.

## FU-6 (P3) Relative `cortex --workspace` — O-S1-04

**Problem.** `run_checks` sets `current_dir(req.workspace)` and also passes `req.workspace.join(rel)`
(`runner.rs:3608-3611`), so a relative workspace is joined twice and every check fails. The same may apply to the
visible-compile call at `runner.rs:2440` [INFERENCE].

**Change.**
- Canonicalize `--workspace` once after CLI parsing in each of `repair`, `locate` and `grant`
  (`bin/cortex.rs:551/1611/1716`).
- A path that does not exist is a usage error.
- `current_dir` is kept (M71).

**Acceptance.** A fail-first CLI test runs `cortex repair` from the workspace's parent with a relative `--workspace`
and gets a real verdict, not "every check fails".

## FU-7 (P3) Lexer-aware `locate::fn_body` — O-S1-11

**Problem.** `locate.rs:111-128` finds `fn NAME(` and counts raw braces, so it is fooled by braces in strings and
comments and by `fn NAME(` inside a comment.

**Change.**
- Reuse the lexer-aware `symbol_body` (`runner.rs:177-216`): make it `pub(crate)` or move it with `scan_code` into a
  shared module.
- If anchors move, update `scripts/v022_refusal_coverage.py:1834-1847`.
- The sibling helpers (`defined_fns`, `test_fns`, `called_in`) stay out of scope and are logged.

**Acceptance.**
- New `#[cfg(test)]` unit tests in `locate.rs`:
  - a `"}"` string in the body;
  - a `// fn f(` comment above the real definition;
  - in both cases `reachable` still sees the callee after the string.
- They fail before the change and pass after.

## FU-8 (P3) `run_ab.py` integrity-abort fixture — O-S1-10

A fixture test drives `run_ab.py` grading with a fake `axon` binary whose verdict lacks a valid completion token or
HMAC. It asserts the run aborts with the integrity error and writes no scored row. The test runs from `gate.sh` next to
the FU-1e self-tests and spawns the script through its interpreter, the same way the `tests/script_spawn` rule does for
Rust tests. No target/ binary default.

## FU-9 (P3) Build-loop skill update — O-S1-16

`/root/.omp/agent/skills/build-loop/SKILL.md` gains the 10 S1 lessons (`tasks/lessons.md` § "S1 — governed repair
episode"). Each lesson is placed in the section it governs, with no duplicate text:
- no stash;
- absolute paths;
- every gate stage before reporting;
- neutral audit wording;
- probe real tasks;
- partial-outcome disk invariant;
- g01 rows land with the fix;
- re-locate the symbol after a splice;
- diff the first prompt against base;
- downstream suites in the stage list.

Out-of-repo file: no commit. The diff is recorded in the end-of-run report.

## FU-10 (R4) Typed-feedback keep/remove criterion — O-S1-18 (fixed before measuring)

R4 is typed JSON feedback for rejected attempts (`crates/axon-cortex/src/feedback.rs`, 369 lines, from 3ffc816b,
015f16b7, fd032a6d).

**Arms (on `hx-test`, same run as FU-11):**
- T1: the candidate head (typed R4).
- T2: the same head plus `B/headroom/ablation/r4-prose.patch`, which renders later-attempt feedback as pre-R4 prose and
  changes nothing else. Built into its own target dir; the patch sha is recorded in the ledger row's purpose.

**Decision rule (pre-registered here and in `PREREG.md` before any T2 run):**
- **KEEP** if either holds:
  - the solve-rate gain Δ(T1−T2) has one-sided cluster permutation p < 0.05 on `claude-haiku-5-5`, and the second
    model's Δ̂ is not below −5 points;
  - Δ̂ ≥ 0 and proposals per verified fix are lower for T1 at p < 0.05 (paired, cluster permutation).
- **REMOVE** otherwise, i.e. no demonstrated capability gain to set against its implementation cost: 369 lines, plus
  the prompt-shape surface that already needed one regress fix (fd032a6d).
- **Execution:**
  - REMOVE → task FU-10r, conditional in this batch: delete typed rendering and keep the prose path. It changes the
    repair digest, so it needs its own receipt regress.
  - KEEP → close O-S1-18 with the numbers.

## FU-11 (eval) Evaluate the batch candidate on `hx-test`

After FU-1…FU-9 are merged to `main` and pushed, run these pre-registered arms, each a `--spend` row:

| Arm | Code | Purpose |
|---|---|---|
| T0 | f26e9506 cortex (fresh baseline, independent of calibration) | baseline |
| T1 | merged head | candidate |
| T2 | merged head + R4 prose patch | FU-10 |

- Primary model `claude-haiku-5-5`; second model `claude-haiku-4-5-20251001`. Every arm runs both models at the k from
  FU-1f.
- Report:
  - the gain criterion FU-1h for T1 vs T0;
  - the FU-10 verdict from T1 vs T2;
  - the per-stratum contamination check;
  - per-task variance.
- Hard-once stays unspent.

---

## Cross-cutting

- Fail-first tests and g01 rows land in the same commit as each fix.
- After any `crates/*/src` commit, in order:
  1. write the pci theme file;
  2. `python3 -B tasks/r51-pci-regen.py`;
  3. `python3 -B scripts/pci_delta.py --check`.
- Gate stages per lane:
  - fmt;
  - refusal coverage;
  - `pci_delta --check`;
  - `claims_gate`/`reference_gate` with `AXON=`;
  - g01 rows touched;
  - `cargo test -p axon-cortex`;
  - clippy `--features ai`;
  - `cargo test -p axon-loop-contracts -p axon-loop -p axon-fabric`;
  - `cargo test -p axon-core --no-default-features --test harness_binaries`.
- Full `./scripts/gate.sh` on the integrated head before merge.
- Deferred work → `tasks/opportunities.md`; lessons → `tasks/lessons.md`.
