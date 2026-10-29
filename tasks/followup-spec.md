# Follow-up spec: S1 follow-ups (held-out headroom set, capability walkers, cortex fixes)

**Status: PARTIAL 2026-10-18 on `main` 791dee95 (FU-2...FU-9 built on `fu/int`; FU-1, FU-10, FU-11 moved to tasks/hx2-spec.md, STOPPED at HX-0, so O-S1-02 stays open). Stamped 2026-10-29 by tasks/docs-truth-spec.md DT-5.**

Reviewed 2026-10-18 (Step 1: conformance audit `local://followup-audit.md` A#1–22, dev-loop/statistics review
`local://followup-devloop.md` D#1–21; dispositions inline as `[REVISED: …]`). Branch `fu/int` (worktree
`/home/cklaus/projects/axon-lanes/fu-int`), base `main` f26e9506. Original draft: commit 0fb92f96.

Source: S1 end-of-run opportunities (`tasks/opportunities.md` O-S1-02, 04, 09, 10, 11, 12, 13, 14, 16, 18) and the
2026-10-18 human decisions (`tasks/repair-spec.md` NH-R1/NH-R2 DECIDED; `tasks/approval-residue-spec.md` H4).
Evidence maps: `local://heldout-map.md`, `local://walkers-bugs-map.md`, `local://approval-map.md`.

Out of scope (separate DRAFT specs written after this batch ships): NH-R1 unattended authorization plus its
enforcement gate (`tasks/unattended-auth-spec.md`), and the R6 trial ledger. **This spec authorizes no unattended
execution.** The settled decisions NH1/H1, H3, KG4, H4 and the Fabric digest channel H2 are not reopened.

## Priority tiers

| Tier | Items |
|---|---|
| P1 | FU-1 headroom held-out evaluation (O-S1-02); FU-2 capability walkers (O-S1-14) |
| P2 | FU-3 drain stop state (O-S1-12); FU-4 restore only our own bytes (O-S1-13); FU-5 non-UTF-8 effect ceiling (O-S1-09 narrow) |
| P3 | FU-6 relative `--workspace` (O-S1-04); FU-7 lexer-aware `fn_body` (O-S1-11); FU-8 `run_ab` integrity-abort fixture (O-S1-10); FU-9 build-loop skill lessons (O-S1-16) |
| R4 | FU-10 R4 keep/remove criterion (O-S1-18), fixed here before any measurement |
| eval | FU-11 evaluate the batch candidate on the frozen held-out set (after merge) |

**Repair digest.** The digest covers `crates/axon-cortex/src/*.rs` and `src/bin/*.rs` (minus `ai.rs`/`label.rs`) plus
`benchmarks/drivers/*.sh`. FU-3, FU-4, FU-6 and FU-7 change it, so the integrated head needs ONE paid receipt
`regress` before merge, with no waiver. That one regress runs after all four land, not once per item. [REVISED: A
sequencing #3.] A second regress is needed only if FU-10r lands. [REVISED: D#19.]

**Sequencing.** [REVISED: added, A sequencing table]
- FU-3 → FU-6 → FU-7 → FU-4 land serially on the cortex files, because g01 `original` strings and refusal-coverage
  anchors match exact text. Refusal coverage and the touched g01 rows are re-run after each.
- FU-2 and FU-5 (axon-core only) run in parallel with them.
- FU-1 code (Python) runs in parallel with them.
- The hx build itself needs FU-7 to have landed, or it must pin the cortex binary it uses (see FU-1c).
- The FU-10 T2 patch is cut against the merged head after FU-3.

---

## FU-1 (P1) Headroom held-out evaluation — O-S1-02

**Problem.** Every existing set is saturated for the incumbent generator (`claude-haiku-5-5` via micode):
- dev: 60/60
- heldout and once: at ceiling
- receipt: 107/115 (ceiling 108)
- hard-dev: 24/25 in scope

A same-code replicate gave 0/115 discordant pairs, and a McNemar gain needs ≥ 6:0 discordant pairs. No candidate can
show a gain on these sets.

Weaker generators are unreachable (`B/scorecard/PREREG-regress.md:162-171`):
- claude-3-haiku and claude-3.5-haiku: 404
- OpenRouter via micode: refused
- llama and qwen: 401

So headroom has to come from harder tasks, not a weaker model.

**Goal.** A new task set `hx`:
- a dev split for development and calibration;
- a frozen test split on which the incumbent solves about 20–80% of tasks;
- enough tasks and programs to detect a meaningful candidate gain;
- protected by digest pins and a spent ledger.

### FU-1a Task classes (mechanically derived; every task single-symbol and in scope)

Cortex replaces one function body per attempt (`earlystop.rs:1-29`). So every hx task is built so that restoring one
function body fixes it. hx reuses the reverse check of `B/hardset/build.py:155-168` and asserts it the other way.

Every class also requires these build checks:
- the broken file is `axon check`-clean, with no E1201 at check time [REVISED: A#18];
- ≥ 2 pristine tests fail (`task_from_broken` rule);
- restoring the body passes every pristine test.

| Class | Construction | Extra build checks |
|---|---|---|
| `multi` | 2–3 independent mutations in ONE function body | each mutation alone fails ≥ 1 PRISTINE test [REVISED: A#16, was "hidden test"] |
| `caller` | mutate callee `g`; its own visible tests are cut from the broken file; grading keeps them; the builder emits `broken_source` [REVISED: A#16] | the visible failure text does not name `g`; `g` is in `reached_by_all` of the failing visible checks |
| `subtle` | type-correct operators: loop/range bound ±1; swap of two same-typed call arguments; read of the wrong same-typed struct field; accumulator init swap; dropped `else` arm value | no visible test fails by a type or parse error |
| `refine` | programs using refinement `where`, struct `where` or `@[verify]`; the mutation surfaces as a refinement or verify failure | failure text matches `verify failed in` or the refinement-violation text (`interp.rs:4886, 4895, 6647`) [REVISED: A#18] |
| `caps` | target is `@[contained]` or capability-narrow, and its natural fix stays narrow ("capability-narrow solving") | pristine `axon caps --fn <symbol>` lists no fs/net/exec kind |

[REVISED: A#10 — the `caps` "workspace data file" clause is dropped. `run_ab` writes only `task["file"]`
(`run_ab.py:645-648`) and `corpus.load` has no extra-files field. Cortex refuses capability widening at R1 admission
(`runner.rs:519-522, 686-688`), so for arm B this class measures solving under a narrow grant, not cheating.]

Known confound, recorded in PREREG [REVISED: A#17]: incumbent early stop counts a proposal as inert when it leaves the
visible failing set unchanged. A partial fix of a `multi` task can look inert, so `multi` headroom partly measures the
early-stop heuristic.

Code locations:
- Mutation operators sit next to the existing ones (`B/heldout/mutators.py`, `B/common.py`).
- A new builder `B/headroom/build.py` writes tasks in `corpus.load` format (`B/ab/corpus.py:278-293`).

### FU-1b Sources and contamination control

- **Stratum D (derived).** Repo programs that no existing corpus uses: the 17 hardset snapshots without tasks, plus
  `traits_demo`. They are public, so contamination is possible. Several were already rejected as "body reveals the
  fix" (`B/hardset/rejected-ideas.json`). [REVISED: D#3 — expect a thin yield.]
- **Stratum C (control).** **20** new programs (minimum 18), written independently for this set [REVISED: D#3, was
  12–16]:
  - the authoring agent works only from short, neutral problem statements;
  - it never sees existing benchmark sources, cortex prompts or results;
  - tests are committed before any defect is derived;
  - the programs live in `B/headroom/programs/`.
- Programs that yield fewer than 3 valid tasks are dropped before the split. [REVISED: D#3]
- **Contamination check: descriptive, not a gate.** [REVISED: D#4 — the original used an unpaired comparison, had low
  power, and its C-only fallback dropped primary power to ≈ 0.39.]
  - Report the per-stratum incumbent rate and the per-stratum Δ with a test-inversion CI.
  - Test the D − C rate difference with a one-sided program-label permutation (10 000 permutations).
  - Report the `contamination-suspect` flag next to the result.
  - The FU-1h criterion stays on the pooled set. The C-only Δ is a secondary row, with its power stated in PREREG.

### FU-1c Split, calibration, freeze

1. **Build the pool and split it by source program.** All tasks of one program go to the same split.
   - The split is a seeded shuffle; the seed is in `B/headroom/PREREG.md` before any model run.
   - It puts **≥ 20 programs in test and ≥ 10 in dev**, by program count, not 50/50.
   - Each test program contributes at most 8 tasks; extra tasks are dropped by seeded draw before any run.
   [REVISED: D#2, D#3]
   - The hx builder's `axon`/`cortex` binaries (used for locate, check and test validity) are pinned. Their sha256 goes
     in PREREG, and they are built from a tree that already contains FU-7. [REVISED: A sequencing #4–5]
2. **Pilot on hx-dev** (unprotected, ≈ $0.45). [REVISED: D#16, new step] Run 20 tasks with the incumbent, h55 k = 1
   and h45 k = 1. Record:
   - cost per task-trial, by outcome;
   - wall time;
   - rate-limit rows.
   `power.py` uses the measured costs.
3. **Calibrate on dev only.**
   - Incumbent = f26e9506 cortex, arm B. Primary model `claude-haiku-5-5`, k = 3.
   - **Cells** = (class, knob). Knob ∈ {multi: 2|3 mutations; caller: depth 1|2; subtle: operator id; refine:
     refine|verify; caps: —}; the final list is in PREREG.
   - A cell is eligible with ≥ 12 dev tasks from ≥ 3 dev programs.
   - Cell rates are empirical-Bayes shrunk (beta-binomial across cells). A cell is kept if its shrunk rate is in
     **[0.3, 0.7]**.
   - **Cross-fit.** Dev programs are split 2-fold (seeded). Cells are selected on fold A and measured on fold B, and the
     reverse. The out-of-fold kept-cell rate, weighted by test cell counts, is the predicted test rate.
   - h45 k = 1 on kept dev cells gives the h45 informativeness check (FU-1h iii).
   - [REVISED: D#5 — the original band [0.2, 0.8] on raw k = 3 rates with an undefined "knob" regressed toward the
     extremes, and its acceptance was true by construction.]
   - Dev tasks with mixed outcomes form `hx-dev` for future development. hx-dev rates regress to the mean; hx-dev is
     never a calibration source. [REVISED: D#7]
4. **Apply the recipe to test by cell membership only.** No test task is run or selected by its outcome.
   - If the kept test pool is too small for FU-1f, build more pool from the same recipe.
   - New programs are split by a new seeded shuffle.
   - New mutation seeds on existing programs inherit that program's split.
   - No program run on dev ever enters test. [REVISED: D#6]
5. **Freeze.**
   - Commit `B/headroom/hx-test.json` and pin its sha256 in `PROTECTED_SETS` (FU-1e).
   - k per model is fixed in PREREG before the freeze. [REVISED: D#12]
   - After the freeze, `PREREG.md` only gets appended result rows.

### FU-1d Grading additions (hx only)

- `run_ab.py grade` records `caps_widened` in every hx row: kinds(`axon caps --json --fn <symbol>` of final) minus
  kinds(the same, of the PRISTINE source). This makes the baseline identical for both arms and excludes tests.
  [REVISED: A#9, was given vs final]
- `verified_clean` on hx (task meta `caps_strict: true`) also requires `caps_widened == []`.
- For arm B, `caps_widened` is structurally `[]` (R1 admission). A non-empty value is a fence-failure alarm: it counts as
  a false success and is reported separately. [REVISED: A#9, D#21]
- Existing sets keep their scoring. Acceptance check: re-score one pinned receipt `b.jsonl` before and after; the
  metrics must be identical. [REVISED: A sequencing #7]
- `run_ab` gains `--grade-axon PATH`, the axon used for grading, so all arms grade with the same axon (FU-11).
  [REVISED: D#14, A#22]

### FU-1e Integrity: pins, spent ledger, hard-once

- **Pins.** `B/protected.py` (new module, imported by both `run_ab.py` and `scorecard.py`) holds `PROTECTED_SETS`, a map
  from set name to corpus sha256. It covers `hx-test` and `hard-once` (`3ebb8d12…ff8c8d6`). [REVISED: A#13 — run_ab
  does not import scorecard]
- **Who must spend.** Only **paid** runs (`run_ab.py:810-811`, generator `real`) on a protected corpus need
  `--spend PURPOSE` and write a ledger row. Oracle and `none` controls are allowed and write nothing. [REVISED: A#13]
- **Ledger** `B/heldout-spent.jsonl`. It is hash-chained.
  - One row per (arm, model) run of all k trials: `{ts, set, corpus_sha256, purpose, repair_digest, patch_sha256,
    cortex_sha256, axon_sha256, grade_axon_sha256, generator, model, k, out, git_head, prev_row_sha256}`.
  - The row is appended before the first model call. [REVISED: D#12]
  - **Second spend refused.** A second row with the same `(set, repair_digest, patch_sha256, model, purpose)` is
    refused. This stops re-rolling a result.
  - **Resume.** `--resume ROW_SHA` continues that row's `out` file. It is allowed while the file has fewer than n·k
    rows, and only with the same binaries. It is not a second spend.
  - **Discard.** `--rerun-of ROW_SHA REASON` discards a run after an infrastructure fault. It writes a new row, and both
    runs are reported.
- **Set names.** `SETS` gains `hx-dev` and `hx-test`. `hx-test` joins `ONCE_SETS`, so two checks apply: ONCE_KEY
  refuses a second scored row per config, and the ledger refuses a second spend. [REVISED: A#12, D#9]
- **Gate stage (every gate, source-only `par` group, seconds, no binary).** [REVISED: D#17–18, A#11 — `B/heldout/
  build.py --check` does not exist. `B/hardset/build.py --check` validates single defects files, runs binaries, and
  always exits 0 (`:362-363`). A byte-exact rebuild cannot match, because `examples_commit` and `spectrum_rank` drift.]
  The stage checks:
  - the sha256 of each protected file against its pin;
  - the ledger hash chain;
  - `corpus.load` of hx-dev, hx-test and hard-once;
  - the B/ self-tests (FU-1e, FU-1d, FU-8, `stats.py`).
- **Builder check (only when builder inputs change).** Builder inputs: `B/headroom/**`, `B/ab/corpus.py`,
  `B/common.py`, `B/heldout/mutators.py`, recorded as a digest in PREREG.
  - `B/headroom/build.py --check` re-validates every task's construction checks with the freeze-pinned binaries.
  - It compares task content with `examples_commit` and `spectrum_rank` excluded.
  - It runs in the builder's self-test, not in every gate.
- **hard-once is not spent in this batch.** Acceptance: the ledger has no hard-once row.
- **Honest limit.** The ledger records spending and stops accidental re-use. It is not a security boundary: a copied
  corpus file bypasses it. `B/headroom/README.md` states this.
- **Logged, not in scope.** `B/hardset/build.py` discards `main()`'s return (`:362-363`) and validates nothing with no
  file given → O-S1-21.

### FU-1f Power analysis (sizes the test split)

**Test.**
- Statistic: the task-weighted mean of the per-task difference in k-trial success rate (candidate − baseline, same
  tasks).
- p-value: one-sided sign-flip test on program sums. Exact enumeration when 2^G ≤ 65 536; otherwise 10 000 seeded Monte
  Carlo permutations.
- 90% CI: inversion of the same test, not a bootstrap.
- All of this lives in `B/headroom/stats.py`, which FU-1b and FU-10 also use, with a self-test in gate. [REVISED: D#8]

**Power simulation** (`B/headroom/power.py`). [REVISED: D#1 — a uniform logit shift adds no program effect and leaves
0/3 and 3/3 tasks unmoved]
- Per-task baseline rates are beta-binomial posterior means of the dev k-trial counts, so no task sits at exactly 0 or 1.
- Effect model: logit shift β + u_g + e_i, where u_g ~ N(0, σ_g²) is a program effect and e_i a task effect. β is set so
  that the mean gain is the MDE (+10 points).
- Power is reported for σ_g ∈ {0, 0.5, 1.0}. The design is sized for power ≥ 0.80 at σ_g = 1.0.
- A second "flip" model is also reported: a fraction f of baseline-failing trials become successes, concentrated in a
  random 30% of programs.

**Design target.** [REVISED: D#2, A#15 — the original "≈150 tasks → 0.85" was arithmetic-inconsistent (0.91 without
clustering) and hid the program-count constraint]
- [EST] G_test ≥ 20 programs, ≤ 8 tasks per program, n_test ≈ 160.
- k = 5 on h55 gives power ≈ 0.88 at τ_b = 0.10 (≥ 0.96 at τ_b ≤ 0.05). k = 1 on h45.
- `power.py` output replaces these numbers and is committed in PREREG before the freeze, subject to the cost cap.

### FU-1g Repeated trials and second generator model

- **k trials.** `run_ab --trials K`:
  - rows carry `trial`;
  - done-rows are keyed by `(id, trial)`;
  - execution is trial-major (all tasks of trial 1, then trial 2…), so a cap or rate-limit stop leaves complete trial
    blocks.
  `analyse.load` and `scorecard.load_rows` key on `(id, trial)` for `TRIAL_SETS = ("hx-dev", "hx-test")` and refuse
  duplicates of that pair. [REVISED: D#9, A#12 — today `analyse.load` silently keeps the last trial, `load_rows`
  refuses repeated ids, and resume is id-only]
- **Model variance.** The report gives per-task between-trial variance and T0's trial-to-trial discordance.
- **Second generator.** `claude-haiku-4-5-20251001` via micode: reachable, priced in `run_ab.py:79-83`, and a different
  model generation. Sonnet 4.5 is stronger and would likely saturate the set. Non-Anthropic generators have no driver
  and are out of scope.
- **Ranker.** Stays `openrouter:anthropic/claude-haiku-5.5` for both generator models.

### FU-1h Gain criterion (pre-registered; NH-R2's hard-once eligibility gate)

[REVISED: D#10 — unit for false successes, an informativeness guard on the second model, and the multiplicity
statement]

A candidate shows **meaningful task-solving improvement** on `hx-test` when all of these hold:
- **(i) Primary model** (`claude-haiku-5-5`): one-sided sign-flip p < 0.05, and Δ̂ ≥ +5 points. The +5 is a practical
  floor and does not bind at the planned SE.
- **(ii) False successes** summed over all n × k primary trials are ≤ T0's.
- **(iii) Second model:** Δ̂₂ > 0, provided h45's kept-cell dev rate is in [0.1, 0.9]. Otherwise (iii) is recorded as
  "uninformative" and the criterion rests on (i) and (ii).

The conditions form an intersection, so the familywise α stays ≤ 0.05. Eligibility to spend hard-once still needs
explicit human approval (NH-R2 = A).

### FU-1 cost and acceptance

[REVISED: D#15, A#14 — $0.0012/task/arm is a ~93%-solved-set figure; at a 50% solve rate it is ≈ 2.5× higher]

**Cost model** (per task-trial at a 50% solve rate; list-price estimate on the OAuth grant; the binding resource is the
grant's rate limit; only the ranker is real OpenRouter spend):
- h55: ≈ $0.003 (success $0.0013, failure $0.003–0.007)
- h45: ≈ $0.018

**Caps:**
- pilot ≈ $0.45
- dev calibration ≤ $5 (h55 k = 3 on the dev pool, plus h45 k = 1 on kept dev cells)
- FU-11 ≤ $25 (the planned design ≈ $16, range $11–22)

Each run sets `--cap-usd`. Concurrent arms share `--spend-root`.

**Wall time.** ≈ 5 h of model time for FU-11 at 3 in flight.

**Acceptance:**
- `hx-dev` and `hx-test` are committed and pinned.
- `PREREG.md` holds the seed, recipe, cells, binary shas, power output, k per model and criteria, committed before the
  freeze.
- Test has ≥ 20 programs and ≥ 150 tasks.
- The **cross-fitted predicted test rate** is in [0.2, 0.8]. [REVISED: D#5]
- The gate stage passes.
- The ledger has no hard-once row.
- Fail-first self-tests cover:
  - refusal of an unspent paid run on a protected corpus;
  - an oracle run on a protected corpus writes nothing;
  - duplicate-spend refusal and resume acceptance;
  - chain-tamper detection;
  - `(id, trial)` keying;
  - `caps_strict` grading (a final file that adds `read_file` to the symbol is not `verified_clean`);
  - `stats.py` against an exact small-G enumeration.

---

## FU-2 (P1) Capability walkers see predicate routes — O-S1-14

**Problem.** The program-wide walkers root only `FnDef.body`, impl-method bodies and `LetDef.value`:
- `program_capabilities` (`crates/axon-core/src/capabilities.rs:901-923`);
- `program_builtin_calls` / `program_effect_rows` (855-899).

A builtin or native call written directly in a `RefineDef.predicate` (named, or the synthetic `__refine_*` from an
inline `where`), a `TypeDef.refinement` or a `FnDef.verify.predicate` is invisible to them.
`check_native_grants` (per fn, 494-523) walks `fndef.body` only.

Existing guards are partial:
- E1209 rejects impure builtins in refinement predicates of type-checked programs.
- `@[verify]` has no purity check.
- G2 and E1203 see ASTs that were never type-checked.

Affected consumers:
- G2 E1402 (`improve.rs:164-178`)
- E1203 import edge (`capabilities.rs:1173-1197`)
- `axon lock` / E1204 (`audit.rs:100-130`)
- G1 purity filter (`main.rs:2015`)
- deploy risk (`main.rs:9123-9180`)
- `--emit-manifest` `effect_union` (`main.rs:8986-9006`) → axon-vm admit
- E1004

S1's `reachable_capabilities` already sees verify predicates, used refinements and struct literals (`TypeHooks`,
`push_fn_hooks`, `push_type`).

**Change (reuse S1's implicit-execution traversal).**
- **Program-wide walkers** (`program_capabilities`, `program_builtin_calls`) root every expression S1's `TypeHooks`
  knows about: every fn and impl-method body and its `verify.predicate`, every `LetDef` value, every `RefineDef`
  predicate, every `TypeDef` refinement. They read the same `TypeHooks::of(program)` table, so there is one definition
  of which items carry implicit code. Rooting every predicate (used or not) over-approximates the S1 walker, which is
  sound for these ⊇ consumers.
- **Per-fn `check_native_grants`** [REVISED: A#7, was one program-wide root list — E1004 is per fn and a free-standing
  predicate has no grant]:
  - walks the fn's body plus its `verify.predicate`;
  - plus the predicates of refinement types and struct refinements its signature and bindings use, via S1's
    `push_fn_hooks` / `push_type` attribution;
  - so a native call in a refinement used by `f` is checked against `f`'s grants.
- **Consolidation rule.** Merge `collect_caps_expr` into the S1 reach walker only if the narrow change shows more than
  the roots list duplicated. Otherwise log it with line counts in `opportunities.md`.

**Shared fixtures (agreement test).** One fixture table in `crates/axon-core/tests/` with these predicate-only cases:
- direct `read_file` / `write_file` in a `@[verify]`;
- in a struct `where`;
- in a named refine;
- in an inline param refinement;
- a native `M::f` in `@[verify]`.

Fixture rules: [REVISED: A#6 — the S1 walker reaches refinements and structs only on use, and `effect_union` speaks
effect rows, not capability labels]
- Each fixture is an un-type-checked parse, so E1209 does not intervene.
- Each fixture USES its predicate: a param of the refined type, a struct literal, a fn carrying `@[verify]`.

Assertions on the four consumers:
1. **G2 diff:** `improve` verify of a pass that adds the predicate → E1402.
2. **Import checker:** a contained importer of the module → E1203.
3. **Manifest emitter:** `program_effect_rows` contains the effect row the kind implies (fs/net → `IO`, exec → `Exec`).
   It is asserted directly, because `build_axmeta_manifest` copies it verbatim (`main.rs:8986`). The codegen
   `--emit-manifest` leg is OUT; decided, no "if". [REVISED: A#8]
4. **S1 reach:** `reachable_capabilities` reports the same capability label as (1) and (2).

The native case is asserted on (1), (2) and (4) only. `program_builtin_calls` records no native module effects even
from bodies, so `effect_union` under-reports native calls everywhere; that is a separate gap → O-S1-20.

Other test work:
- One unit test per root kind, next to `program_capabilities_sees_impl_method_bodies` (2620).
- One E1004 test for a native call in a used refinement.
- g01: one mutation per new root kind in `scripts/v022_g01_mutations.py`, in the same commit.

**Acceptance.**
- (1)–(3) fail before the change and pass after; (4) passes before and after.
- Lane stages include `cargo test -p axon-core` (default features). [REVISED: A#8]
- Logged, not in scope: `@[verify]` has no purity check → O-S1-19 (a language-behaviour change).

---

## FU-3 (P2) A stopped check drain is never complete output — O-S1-12

**Problem.**
- `drain_bounded` (`crates/axon-cortex/src/runner.rs:3742-3771`) leaves its read loop on `wait_readable == false`
  (stop set) exactly as on EOF.
- `join_within` (`axon-jobctl/src/lib.rs:455-485`) sets stop after `DRAIN_GRACE` and returns `Some(partial)`.
- `joined_output` (3722-3730) accepts that partial capture as complete.
- Generator side: `generate.rs:660` `.map(Option::unwrap_or_default)` turns a DETACHED drain (`None`) into empty bytes.
  Those bytes are parsed as an empty proposal, not reported as a lost read. [REVISED: A#1 — found on the same lines]

**Change.**
1. **Reproduce first.**
   - E2E: a registered check wrapper, following the `cortex-check-registry/1` pattern of
     `r2_a_timed_out_check_is_killed_as_a_group_and_the_loop_goes_on` (`tests/cli.rs:2439`). The wrapper's real
     `axon test` runs under a `setsid` grandchild that holds the pipes past `DRAIN_GRACE` and is still writing.
   - Record today's verdict for it (pass, or fail by missing completion tokens) in the commit.
   - Unit test in `runner::drain_tests` (~4465): the write end is held open, stop is set, and the drain must be marked
     stopped.
2. **Check side.**
   - The drain returns an explicit stopped state.
   - `joined_output` maps it to `BoundExceeded::unread()`, keeping the anchor `'_ => Err(BoundExceeded::unread()),'`
     byte-identical.
   - An incomplete check yields no verdict and never passes.
   - Stated consequence: a check whose wrapper leaves a descendant holding its pipes past `DRAIN_GRACE` never gets a
     verdict, even if its output happened to be complete. [REVISED: A#2]
3. **Generator side.** [REVISED: A#1 — BLOCKER. "Reject every stopped generator drain" contradicted the NH1/KG2
   leftover contract and `kg2_a_leftover_holding_the_generators_pipes_does_not_stall_the_loop` (`tests/cli.rs:2225`),
   whose complete answer always arrives via a stopped drain.]
   - A DETACHED drain → `GenerationFailure::Unavailable("… output could not be read to EOF")`, never empty bytes.
   - A drain that was stopped while data was still pending (a final non-blocking read after stop returns bytes) →
     incomplete, rejected the same way.
   - A drain stopped while idle after the generator exited keeps its bytes (the NH1 leftover case).
   - Update M3602's `original` and the `p.take(cap)` refusal anchor if their text moves.
4. **g01 rows** for the stopped state in both drains and for the detached → Unavailable mapping.

**Acceptance.**
- The reproductions fail before the change and pass after.
- The `r2_*` and `kg2_*` CLI tests are unchanged and green, including the KG2 leftover test.

## FU-4 (P2) Restore only bytes this run wrote — O-S1-13

**Problem.**
- `settle_artifact` (`crates/axon-cortex/src/bin/cortex.rs:1581-1594`) writes the pre-repair original back on any
  `accepted_artifact` error, including "it changed after acceptance" (1566-1571). That overwrites another actor's edit.
- It ignores a failed write.
- The restore at 1161-1163 (`code != 0 && != 27`, unconditional `let _ = write`) does the same. [REVISED: A#4]

**Change.** All restore sites in `bin/cortex.rs` (1042, 1161, 1581) go through one helper, using the decision table
below. [REVISED: A#3 — the original rule contradicted "a read-back error still restores"]
- **We-wrote set.** The episode's `PatchApplied.after_digest` values (compared through `content_digest`; `axc1:` prefix)
  plus the adjudicated bytes. No new tracking. [REVISED: A#4]

| File state at restore | Action |
|---|---|
| (a) read OK, sha ∈ we-wrote | restore the original; report |
| (b) read OK, bytes equal the original | nothing to do |
| (c) read OK, sha ∉ we-wrote | leave in place; the reason says "changed by another writer; not restored" |
| (d) read error | do not write (we cannot prove the bytes are ours); the reason says "could not be read; not restored" |
| (e) restore write fails | the reason carries the io error |

- Exit codes are unchanged: 22 stays 22.
- The between-attempt restores in `runner.rs` (1790, 2767) run right after the run's own write. They are audited
  against the same table and adopt the helper if it applies without changing the walk; otherwise the gap is logged.
- **Rework:**
  - `a_refused_settle_restores_the_file_as_found` (1800): case 1 (external bytes after acceptance) now asserts the
    bytes SURVIVE; case 2 (our accepted bytes plus a kept-Err put-back) still restores.
  - Rework g01 M3714 so its kill test exercises case (a).
  - Re-verify the `original` strings of M3708, M3710, M3712, M3713, M3715 and M3716 (M3716 =
    `a_restore_failing_between_attempts_still_ends_with_a_verdict`).

**Acceptance.**
- Fail-first unit tests for (c) and (d): today the file is overwritten.
- A test with an unwritable target reports (e).
- The r5_* CLI tests stay green.

## FU-5 (P2, narrow) Non-UTF-8 `AXON_ALLOWED_EFFECTS` denies every effect — O-S1-09

**Problem.** [REVISED: A#5 — the dangerous reader is `main.rs:6747`, which every cortex check goes through (`runner.rs`
passes `--completion-key-stdin`); axon-rt already refuses any set value (`axon-rt/src/lib.rs:2131`) and needs no
change]
- `interp.rs:7566` maps `VarError::NotUnicode` to "unset":
  - with no scope vars set → no ceiling;
  - with scope vars set → `all_effects: true`.
- `main.rs:6747-6759` (`cmd_test --completion-key-stdin`) maps any `Err` to every grantable effect except Exec, then
  rewrites the variable as valid UTF-8, so the interpreter never sees the bad value. A set ceiling fails open on both
  paths.

**Change.**
- Exactly these two readers change. NotUnicode maps to the EMPTY set (deny every effect) plus one stderr warning.
- At 6747 the bad value is never widened to the grantable set; the rewrite writes the empty set.
- Unset keeps today's semantics. Pinned by `cli_run.rs:24175` (`ambient_effect_ceiling_env_var_is_actually_enforced`),
  `cli_run.rs:30871` and `tests/ambient_scope.rs`.

**Acceptance.** Fail-first CLI tests with the variable set to invalid UTF-8 (`OsStr::from_bytes`):
- `axon run` of a `println` program is refused with the sandbox-violation exit;
- `axon test --completion-key-stdin` of a test that prints is refused.

Lane stages include `cargo test -p axon-core`.

The remaining O-S1-09 items (native-module scopes, `@[contained]` closures stored in module `let`s) belong to the NH-R1
enforcement-gate spec.

## FU-6 (P3) Relative `cortex --workspace` — O-S1-04

**Problem.** A relative workspace is joined twice in two places, so every check fails [REVISED: A#19 — the 2440 site is
confirmed, no longer [INFERENCE]]:
- `run_checks` sets `current_dir(req.workspace)` and passes `req.workspace.join(rel)` (`runner.rs:3608-3611`);
- the visible compile does `abs = workspace.join(..)` (2385), then `run_axon_check(&abs, Some(&workspace))` (2440).

**Change.**
- Canonicalize `--workspace` once after CLI parsing in `repair`, `locate` and `grant` (`bin/cortex.rs:551/1611/1716`).
  A missing path is a usage error.
- Canonicalize a relative `--axon` the same way. [REVISED: A#19]
- M71's text (`cmd.current_dir(req.workspace)\n .arg("test")`) stays byte-identical.

**Acceptance.** A fail-first CLI test runs `cortex repair` from the workspace's parent with a relative `--workspace`
and gets a real verdict.

## FU-7 (P3) Lexer-aware `locate::fn_body` — O-S1-11

**Problem.** `locate.rs:111-128` finds `fn NAME(` and counts raw braces, so it is fooled by braces in strings and
comments and by `fn NAME(` inside a comment.

**Change.**
- Reuse `symbol_body` (`runner.rs:177-216`): make it `pub(crate)`, or move it with `scan_code` into a shared module.
- A lex error (`scan_code` → `Err`) yields no body, hence no reach. This is stated as a behaviour change.
  [REVISED: A#20]
- Update refusal-coverage anchors `scripts/v022_refusal_coverage.py:1834-1847` (runner) and 2542-2547 (`locate.rs`,
  including the `fn fn_body` signature). [REVISED: A#20]
- The sibling helpers (`defined_fns`, `test_fns`, `called_in`) stay out of scope and are logged.

**Acceptance.**
- `#[cfg(test)]` unit tests in `locate.rs`, failing before and passing after:
  - a `"}"` string in the body: `reachable` still sees the callee after the string;
  - a `// fn f(` comment above the real definition.
- After FU-7, `B/hardset/build.py --check` on every committed defects file still validates. In-scope ranks must stay
  ≤ CANDIDATES.

## FU-8 (P3) `run_ab.py` integrity-abort fixture — O-S1-10

A fixture test drives `run_ab.py` grading with a fake `axon` whose verdict lacks a valid completion token or HMAC.
- It asserts the run aborts with the integrity error and writes no scored row.
- It runs among the B/ self-tests in the FU-1e gate stage and lands after the other `run_ab.py` changes (same file).
- No target/ binary default.

## FU-9 (P3) Build-loop skill update — O-S1-16

DONE before Step 2 (out-of-repo, no commit). `/root/.omp/agent/skills/build-loop/SKILL.md` gained the 10 S1 lessons in
the sections they govern:
- **Done means the full gate:** every gate stage plus downstream suites, merged into one rule.
- **Isolation:** no stash; absolute paths in every tool call.
- **Implementation review loop:** neutral audit wording.
- **Verification bar:** g01-style rows land with the fix.
- **New paragraphs:** "Partial outcomes state their bytes" and "Generated text spliced into code is re-located".
- **Gates:** "Probe before a paid run" (real tasks, plus a first-request diff against base).

The diff goes in the end-of-run report.

## FU-10 (R4) Typed-feedback keep/remove criterion — O-S1-18 (fixed before measuring)

R4 = commits 3ffc816b (typed `Feedback`: `feedback.rs` +334, `runner.rs` −172/+…, `generate.rs` rendering), 015f16b7
and fd032a6d. It replaced a 2 KiB prose blob (`MAX_FEEDBACK_BYTES`) and its renderers.

**Arms** (on `hx-test`, in the FU-11 run):
- T1: the merged head (typed R4).
- T2: the merged head with R4 reverted. [REVISED: A#21 — a render-only ablation would test neither the gathering's 369
  lines nor a real prose path, since the tree has none]
  - The revert is of 3ffc816b, 015f16b7 and fd032a6d's feedback parts, keeping later non-R4 changes such as fb546253's
    outside-symbol note.
  - It sits on branch `fu/r4-ablation`, and its patch sha is in PREREG and in the ledger key before any T2 run.
  - T2 must pass the hidden-check sealing tests (pre-R4 code filtered `rank::mentions(l, hidden)` too) and its own
    `cargo test -p axon-cortex`.

**Decision rule** (pre-registered here and in `PREREG.md` before any T2 run). [REVISED: D#11 — two OR routes at 0.05
inflated α to ≈ 0.10, and route 2 was undefined]
- **KEEP** if either route holds:
  - Route 1: Δ(T1 − T2) one-sided sign-flip p < **0.025** on h55, and h45's Δ̂₂ ≥ −5 points. The guard's false-veto
    rate at Δ₂ = 0 is ≈ 0.19 at k₂ = 1, stated here.
  - Route 2: efficiency. Per task, take the mean proposals over verified trials in each arm, over tasks with ≥ 1
    verified trial in both arms; d_i = T2 − T1. One-sided sign-flip by program, p < **0.025**, and Δ̂ ≥ 0 on solve rate.
- **REMOVE** otherwise: no demonstrated capability gain to set against R4's implementation cost (the module, the
  runner wiring, and a prompt-shape surface that already needed one regress fix, fd032a6d).
- **Power:** MDE₈₀ ≈ 7 points. The feedback-off ablation lost only 2/115. A REMOVE verdict means "no gain of ≥ ~7
  points shown", not "no effect", and the report says so.
- **Execution:**
  - REMOVE → task FU-10r: land the `fu/r4-ablation` revert on main, update the refusal-coverage anchors (2756-2762,
    1840-1841) and rows M3717-M3719, then run a second receipt regress (digest change).
  - KEEP → close O-S1-18 with the numbers.

## FU-11 (eval) Evaluate the batch candidate on `hx-test`

Runs after FU-1…FU-9 are merged to `main` and pushed. Arms, each its own `--spend` ledger row:

| Arm | cortex (and its `--axon`) | Purpose |
|---|---|---|
| T0 | f26e9506 (fresh baseline, independent of calibration) | baseline |
| T1 | merged head | candidate |
| T2 | merged head + R4 revert | FU-10 |

[REVISED: D#13–14, A#22]
- **Harness and grading.** Every arm runs the merged head's `run_ab.py` and grades with the merged head's axon
  (`--grade-axon`). Each arm's cortex uses its own tree's axon.
- **Commit fields.** Row meta records `cortex_commit` from the binary's build tree, separately from the harness
  `commit`; scorecard checks `cortex_commit` against that field.
- **Concurrent arms.** Per model, T0, T1 and T2 run concurrently: three `run_ab` processes at `--parallel 1` each, so 3
  tasks are in flight. They use the same seeded order, trial-major, and a shared `--spend-root`. h55 runs first, then
  h45.
- **Infra failures.** A trial with `rate_limited`, `usage_partial` or an infra failure is re-run once inside the same
  ledger row. If it fails again, that (task, trial) is dropped for all arms of that model and counted. The analysis uses
  only trial blocks complete in all three arms.
- **Models and k.** Primary `claude-haiku-5-5` at k = 5; second `claude-haiku-4-5-20251001` at k = 1, or per `power.py`.
- **Report:**
  - FU-1h for T1 vs T0, plus Δ̂, the 90% CI, the realised MDE₈₀, and a two-sided interval as a regression check;
  - the FU-10 verdict from T1 vs T2;
  - the contamination row;
  - per-task variance;
  - realised T0 rate. If it falls outside [0.15, 0.85], the paired test still stands and the shortfall is reported with
    the realised power.
- **Expected outcome.** FU-2…FU-9 target correctness, not generation quality, so a null T1 − T0 is expected. The verdict
  is then "not shown", never "no effect". [REVISED: D#20]
- **hard-once** stays unspent.

---

## Cross-cutting

- Fail-first tests and g01 rows land in the same commit as each fix.
- After any `crates/*/src` commit, in order:
  1. pci theme;
  2. `python3 -B tasks/r51-pci-regen.py`;
  3. `python3 -B scripts/pci_delta.py --check`.
- **Lane stages by touched paths.** [REVISED: D#19, A#8]
  - `crates/axon-cortex/**`:
    - fmt
    - refusal coverage
    - `pci_delta --check`
    - `claims_gate` / `reference_gate` with `AXON=`
    - g01 rows touched
    - `cargo test -p axon-cortex`
    - clippy `--features ai`
    - `cargo test -p axon-loop-contracts -p axon-loop -p axon-fabric`
    - `cargo test -p axon-core --no-default-features --test harness_binaries`
  - `crates/axon-core/**`: the same list, plus `cargo test -p axon-core` (default features).
  - `B/**` only:
    - the B/ Python self-tests
    - `scorecard selftest` / `report --check`
    - `check_docs`
    - the FU-1e gate stage
    - `fence_sweep` if the corpus format changed
  - Out-of-repo: none.
- **Integrated head.** Runs full `./scripts/gate.sh` once (~27–32 min); it is re-run only if a commit lands after it.
  The receipt regress (≈ $0.14, ≈ 5 min model wall) runs once on the integrated head.
- **Logged** (`tasks/opportunities.md`):
  - O-S1-19 `@[verify]` purity;
  - O-S1-20 native effects missing from `effect_union`;
  - O-S1-21 `hardset/build.py` exit code and no-file no-op;
  - plus anything new found during the build.
