# Held-out evaluation sets

Two task sets that Cortex has never been tuned on. They use the dev corpus's format
(`../ab/corpus.json`, written by `../ab/corpus.py`'s `merge`), so
`../ab/run_ab.py` arm A and arm B run them unchanged:

```
python3 ../ab/run_ab.py heldout.json --arm B --generator real --out results/<run>/b.jsonl ...
python3 ../ab/run_ab.py once.json    --arm B --generator real --out results/<run>/b.jsonl ...
```

| set | file | tasks | sha256 |
|---|---|---|---|
| `heldout` | `heldout.json` | 47 (0 real, 47 planted) | `2b977f7d5cbede6fed3ce59d85b871f9cd6a13b2477fe3379e74a82b4987d3fa` |
| `once` | `once.json` | 30 (planted) | `3c66ddd64c0af066df6af0cccba11e8b03c542eb80141ff85070490d4d3b995b` |

Built at commit `c6a271d7` by `python3 build.py` (deterministic, no randomness;
same tree and axon binary, same bytes).

## Rules of use

1. **No tuning code reads these sets.** Not locrank, not DecisionBench
   (`crates/axon-reflex/benchmarks/decisions`), not prompt or ranker work, not
   any threshold, candidate count, or mutator choice. Anything tuned is tuned on
   the dev corpus (`../ab/corpus.json`). If a tuning script ever needs a file
   path from here, that is a leak; fix the script.
2. **Every run on them goes through the scorecard**
   (`../scorecard/scorecard.py add --set heldout|once ...`), so each use is
   visible in `../scorecard/history.jsonl`. A run that is not in the history did
   not happen.
3. **`heldout`** is for repeated comparison of Cortex configurations. Each run
   costs it a little independence (results are seen), so look at the scorecard
   numbers, never at individual task transcripts, when deciding what to change.
4. **`once`** is run ONCE per release candidate that has already won on dev and
   heldout, as the final check. Its number is reported, not optimised; if it is
   used to choose between configurations it stops being held out, and a new
   `once` set must be built from files no set has used.
5. The sets are frozen. Rebuilding changes the sha256 above; a rebuilt set is a
   new set and its runs do not compare with older ones.

## How the tasks were made

### 1. Real bugs from git history (`mine.py`, report in `mining.json`)

Every (commit, `.ax` file) pair in `git log -M --diff-filter=MR HEAD -- '*.ax'`
is a candidate. A pair qualifies only if: the fixed file has >= 2 `@[test]`
functions; exactly one non-test function body changed and nothing else outside
tests did; that function existed before (a fix, not an addition); the HEAD file
still has the fixed body and passes every test; the file is not a dev-corpus
source; reverting the fix into the HEAD file passes `corpus.py`'s `make_task`
validity checks (>= 2 tests fail, the arm-A workspace still compiles and fails
the visible test, the hidden check passes on the pristine file); and `cortex
locate` ranks the fixed function within run_ab's `--candidates` (6). Each pair
records the first rule it breaks. The commit subject (`fix`, `bug`, ...) is
recorded but does not qualify a pair: the behavioural witness does.

Result: **0 of 75 pairs qualify** (47 commits).

| reject reason | pairs |
|---|---|
| no `@[test]` functions in the file | 60 |
| no non-test function body changed (tests, comments or declarations only) | 10 |
| change spans more than one function | 3 |
| only one `@[test]` function (needs a visible and a hidden check) | 2 |

The repository's `.ax` history is mostly compiler fixtures without tests and
whole-file rewrites; no commit fixes one tested function. So both sets are
topped up with planted defects (`origin: "planted"` on every task; a real task
would carry `origin: "real"` and its `commit`).

### 2. Planted defects in unseen files (`build.py`, `mutators.py`)

**Files.** Pool = every tracked `.ax` file that is not a dev-corpus source, is
green at HEAD, and has >= 2 tests and >= 1 non-test function; minus Cortex's own
files (`crates/axon-cortex/fixtures`, `examples/cortex`) and the
`r51_s0b` harness-semantics fixtures. Functions whose whitespace-normalised
body is a copy of a dev-corpus function are skipped (64 such). Each file's
EXPOSURE records what any earlier benchmark did with it:

* `none`: never named by any benchmark;
* `decisions`: DecisionBench builds type-check (axbool) items from it; no
  repair task, no Cortex call;
* `loc-0920`: `../localization-2026-09-20.json` planted `../common.py` classes
  in it and measured Cortex's locator on them (no repair, no tuning).

| pool file | exposure | valid pool tasks | set |
|---|---|---|---|
| examples/asi/budget_audit.ax | none | 22 | heldout |
| examples/asi/taint_flow.ax | none | 3 | heldout |
| examples/stdlib/constraint.ax | decisions | 1 | heldout |
| examples/stdlib/plan.ax | loc-0920 | 15 | heldout |
| examples/stdlib/principal.ax | decisions | 1 | heldout |
| examples/stdlib/principal_mint.ax | loc-0920 | 8 | heldout |
| examples/stdlib/reward.ax | decisions | 10 | heldout |
| examples/stdlib/tainted.ax | loc-0920 | 1 | heldout |
| examples/stdlib/trace.ax | loc-0920 | 35 | heldout |
| examples/asi/rank.ax | none | 1 | once |
| examples/asi/safety_stack_demo.ax | none | 3 | once |
| examples/stdlib/asi_prelude.ax | none | 43 | once |
| examples/stdlib/safe_action.ax | decisions | 12 | once |
| examples/stdlib/source.ax | loc-0920 | 3 | once |
| examples/stdlib/world.ax | loc-0920 | 6 | once |
| examples/asi/risk_monitor.ax, examples/fintech/ledger.ax, examples/modular/scorelib.ax, examples/feature_tour.ax, examples/should_fail_test.ax, examples/stdlib/arrays.ax, examples/stdlib/uncertain.ax | | 0 | neither: no mutant made >= 2 tests fail with the arm-A workspace still valid |

The two sets share no file and no function body (files with shared bodies are
on the same side). No file of either set is a dev-corpus source.

**Classes.** The dev corpus's six (operator, constant, boolean, argswap,
drop-stmt, comparison) are reused, made site-aware (k-th site, not only the
first). Eight new ones (`mutators.py`): swap-branches, flip-comparison (a
comparison's direction reversed), wrong-var (one use of a name replaced by
another in scope), wrong-field (one `.field` read replaced by another field of
the same type), float-constant, drop-not, string-typo, operand-swap (`-`, `/`,
`%` operands swapped). float-constant and string-typo yielded no valid task in these files, nor did
the dev class argswap.

**Pool.** For each pool function x class, every site (up to 16) whose mutant
passes `corpus.py`'s `make_task`: 1540 (file, function, class) candidates, 164
valid tasks.

**Selection.** Round robin over classes (new classes first), least-used file
for the class first, then least-used function; at most 3 per (file, class) and
4 per function; no duplicate (original, mutant) body pair; and the defective
function must be in `cortex locate`'s top 6 on the broken file (arm B tries
only those names; a ranker can reorder but never add one, so a defect outside
them is unreachable for every Cortex configuration). `spectrum_rank` records
that rank on every task. One candidate (principal_mint.ax, rank 7) was dropped
by this rule.

### Counts

`heldout` (47; 31 in new classes; 16 functions in 9 files):

| class | n | | class | n |
|---|---|---|---|---|
| wrong-var | 10 | | operator | 4 |
| swap-branches | 6 | | constant | 4 |
| flip-comparison | 6 | | comparison | 4 |
| wrong-field | 5 | | boolean | 2 |
| operand-swap | 4 | | drop-stmt | 2 |

Exposure: none 11, decisions 10, loc-0920 26. Spectrum rank of the truth:
1: 25, 2: 12, 3: 8, 6: 2. Cheat subset (meta): 10 tasks.

`once` (30; 21 in new classes; 12 functions in 6 files):

| class | n | | class | n |
|---|---|---|---|---|
| wrong-var | 8 | | constant | 3 |
| flip-comparison | 6 | | wrong-field | 1 |
| operator | 6 | | drop-not | 1 |
| swap-branches | 5 | | | |

Exposure: none 14, decisions 7, loc-0920 9. Spectrum rank of the truth:
1: 22, 2: 4, 3: 2, 4: 2. Cheat subset (meta): 6 tasks.

## Validity (zero-cost dry runs, `results/dryrun/`)

`run_ab.py` with the zero-cost agents, axon/cortex built at `c6a271d7` without
`--features ai`, no `OPENROUTER_API_KEY`:

| run | heldout (47) | once (30) |
|---|---|---|
| B-oracle (`literal:` original body) | **47/47 verified**, 0 false successes | **30/30 verified**, 0 false successes |
| B-none | 0 verified, 0 claims (`needs_input`) | 0 verified, 0 claims |
| A-oracle | 47/47 verified | 30/30 verified |
| A-noop | 0 verified, 0 claims | 0 verified, 0 claims |
| A-cheat (visible test -> `assert(true)`) | 0 verified, 47/47 false successes | 0 verified, 30/30 false successes |

Every task also passed `make_task` at build time: pristine file green, broken
file type-checks with >= 2 failing tests (visible = first, hidden = last), the
arm-A workspace without the hidden check type-checks and fails the visible test.

### Compile-caught tasks

A checker fix can make a planted defect a compile error. The set stays frozen
(it is not rebuilt, so its sha256 does not change), and the task is recorded in
`../fence_sweep.py` `COMPILE_CAUGHT`. The sweep confirms the refusal on every
gate run instead of sweeping the task.

| task | from | why |
|---|---|---|
| `h-wrong-var-04` | `128d70be` | its mutant `arr_push(&t.results, t)` pushes a `Trace` into an `[i64]`. A builtin's `T` was a wildcard before this commit; now the checker refuses the file with E0102. |

From that commit on, the broken file no longer type-checks, so `cortex grant` exits 22 and every arm, oracle
included, scores the task as not verified. A heldout score is therefore at most 46/47, which is one task below runs
on older binaries. Configurations run on the same binary still compare with each other.

## Files

* `mine.py`: real-bug miner; `mining.json`: its report (every pair and reason).
* `mutators.py`: site-aware dev classes and the eight new classes.
* `build.py`: pool, selection, writes both sets.
* `heldout.json`, `once.json`: the sets.
* `results/dryrun/`: the zero-cost validity runs above.
