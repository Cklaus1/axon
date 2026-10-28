# Hard sets: context-dependent and out-of-scope defects

Two task sets built because Cortex and the plain agent were both close to the
ceiling on `../ab/corpus.json`, `../heldout/heldout.json` and
`../heldout/once.json`. Those sets planted defects that the broken body shows
by itself (operator swaps, off-by-one errors, flipped comparisons). Here every
in-scope defect is still ONE function body, but the fix needs a fact from
OUTSIDE that body. `hard-dev` also has out-of-scope tasks that no single body
edit can repair. These test whether Cortex fails honestly.

The format is the dev corpus's (`../ab/corpus.py` `merge`/`load`), so
`../ab/run_ab.py` arm A and arm B run them unchanged:

```
python3 ../ab/run_ab.py hard-dev.json --arm B --generator real --out results/<run>/b.jsonl ...
```

| set | file | tasks | sha256 |
|---|---|---|---|
| `hard-dev` | `hard-dev.json` | 38 (25 in-scope, 13 out-of-scope) | `f74526b19fa1aaec902cff77668e299f174dbcb90c53d089ec99dbcd18bb0c5b` |
| `hard-once` | `hard-once.json` | 19 (in-scope) | `3ebb8d12211168d1fe11809badad7345532e7ba01a825b3c1a9131889ff8c8d6` |

Both sets were built at commit `b5c4dfa1` by `python3 build.py`. The build is
deterministic: the same tree and axon binary give the same bytes (two
consecutive builds gave identical sha256).

**The pool is smaller than planned.** The target was about 60 in-scope tasks.
The pool gave 44 honest ones, and none of them is a real historical bug (see
below). We did not add trivial mutations to reach the target. `hard-once`
therefore has 19 tasks, not about 30.

## Rules of use

1. **No tuning code reads `hard-once`.** That covers locrank, DecisionBench,
   prompt or ranker work, and any threshold or candidate count. `hard-dev` MAY
   be used for tuning. Once it has been, its numbers are dev numbers, not
   held-out numbers.
2. **Every run on these sets goes through the scorecard**
   (`../scorecard/scorecard.py add --set hard-dev|hard-once ...`). Each use is
   then visible in `../scorecard/history.jsonl`. A run that is not in the
   history did not happen. Zero-cost validity dry runs (`results/dryrun/`) are
   the exception, as they are for heldout.
3. **`hard-dev`** is for repeated comparison of Cortex configurations and for
   tuning. Each run costs it some independence. When deciding what to change,
   prefer the scorecard numbers over individual transcripts.
4. **`hard-once`** is run ONCE per release candidate that has already won on
   dev, heldout and hard-dev, as the final check. Its number is reported, not
   optimised. If it is used to choose between configurations, it stops being
   held out, and a new set must be built from files that no set has used. The
   scorecard refuses a second `hard-once` score for the same candidate binary
   (`ONCE_SETS`).
5. The sets are frozen. Rebuilding changes the sha256 above. A rebuilt set is
   a new set, and its runs do not compare with older ones.
6. **Out-of-scope tasks never count as verified or unverified.** The scorecard
   reports them apart (`metrics.out_of_scope`):
   * `honest_failure`: no claim, and the file left unmodified. This is
     Cortex's correct outcome.
   * `fixed`: verified anyway. The plain agent may legitimately do this, since
     it can edit two functions or a signature.
   * `no_claim`, and `false_success`. A false claim on an out-of-scope task
     still counts in the run's `false_success`.

## Sources

**File pool.** The pool is every `examples/**/*.ax` file not used by
`../ab/corpus.json`, `../heldout/heldout.json` or `../heldout/once.json`: 138
of 182 files. A file P belongs to this (hard) lane iff
`int(sha256(P), 16) % 2 == 0`, which gives 81 files. The other half belongs to
a different set. Of the 81:

* 20 have no function besides `main` (or tests), so there is nothing to
  repair.
* 10 do not type-check under the interpreter CLI. They need modules, the
  native build, or they are deliberate refusal demos.
* 9 have functions whose behaviour the interpreter cannot pin down. The
  test-writers reported model calls, inline asm or port I/O, BPF helpers, or
  property-test-only files.
* 42 were snapshotted.

**Snapshots** (`src/`, commit `1e8e2f61`, committed alone and before any
defect). Each snapshot is the original file's bytes followed by appended
`@[test]` functions, 216 tests in all. The tests describe the behaviour that
comments, names, types and `main`'s documented outputs state, and every one
passes on the pristine copy. `examples/` was never edited. The defects, the
builder and the sets came later, in `b5c4dfa1`. That commit order is the
evidence that the tests were not written to fit the defects.

**Real bugs** (`mining.json`). Every commit that touched a snapshot's original
(`git log -M --follow`) was replayed with `../heldout/mine.py`'s rules, with
the snapshot's tests standing in for the missing ones. **0 of 49 pairs
qualify**:

| reject reason | pairs |
|---|---|
| file added by the commit | 42 |
| layout or comment change only | 3 |
| the changed function is `main` | 2 |
| no function body changed (declarations only) | 1 |
| change spans more than one function | 1 |

Of the 60 pairs `../heldout/mining.json` rejected only for "no `@[test]`
functions in the file":

* 26 are not under `examples/`;
* 20 are in the other lane's half;
* 14 are in this lane's half. 7 of those 14 touch snapshotted files and are
  among the 7 non-"added" rows above. The other 7 touch files with no
  testable function (`interactive/approval_agent.ax`,
  `interactive/guess.ax` x2, `asi/persistent_bandit.ax` x3,
  `kernel/hello_kernel_slice1.ax`).

Adding tests did not turn any of them into a single-function fix.

**Planted defects** (`defects/*.json`). These are hand-written. Each spec
names the outside fact the fix needs (`context`), or, for out-of-scope specs,
why no single body edit can repair it. The authors rejected 56 ideas because
the body alone revealed the fix, fewer than 2 tests failed, or the slip was
unrealistic. `rejected-ideas.json` records all 56 with their reasons.

## Checks every task passed (`build.py`, mechanically)

1. The edits apply exactly once, outside test blocks, to a green snapshot.
2. In-scope: the broken file equals the snapshot with only `symbol`'s body
   replaced, and `symbol` is not `main` or a test. Out-of-scope: the broken
   file is NOT any one-body replacement, and putting back the pristine body of
   ANY single function still leaves a test failing.
3. `../ab/corpus.py` `task_from_broken` (`make_task`'s checks on a whole
   file):
   * the broken file type-checks and has at least 2 failing tests;
   * the named visible and hidden tests are both failing;
   * the arm-A workspace without the hidden check type-checks and fails the
     visible test;
   * the hidden check passes on the pristine file.
4. In-scope: `cortex locate` ranks `symbol` within run_ab's `--candidates`
   (6).
5. No duplicates. This means no repeated (file, broken file) pair, and no
   repeated (pristine body, broken body) pair across files. One spec,
   `options.ax::safe_div`, was dropped because it was identical to
   `error_handling.ax::safe_div`, and removed from `defects/` so `--check` is
   clean.

`corpus.py` was extended, and the existing corpora rebuild byte-identically:

* `task_from_broken(f, src0, symbol, class, how, broken, visible=None,
  hidden=None)` runs `make_task`'s checks on a whole broken file.
* `merge` stores a `broken_source` field (the whole broken file) when the
  defect is not inside `symbol`'s body. `load` rebuilds `broken` from it.

Extra task fields: `scope` (`in`|`out`), `shape`, `context`, `origin`
(`planted`), `example` (the `examples/` original) and `spectrum_rank`.
`run_ab.py` ignores them. The scorecard reads `scope`.

## Defect shapes

| shape (in-scope) | what the fix needs from outside the body | hard-dev | hard-once |
|---|---|---|---|
| documented-constant | a value or convention stated in a comment, doc line, `main`'s documented output, or a sibling | 8 | 13 |
| caller-contract | what callers pass or expect (lambda parameter order, documented call results) | 7 | 2 |
| struct-field | which field of a type definition the body must read | 5 | 0 |
| param-meaning | which parameter is which, from the signature or the callers | 2 | 4 |
| enum-convention | an integer-coded level convention defined elsewhere in the file | 2 | 0 |
| sibling-unit | a unit or scale defined by a sibling function (ROAS x100) | 1 | 0 |
| **in-scope total** | | **25** | **19** |

| shape (out-of-scope, `hard-dev` only) | n |
|---|---|
| two-functions (two bodies wrong; each has its own failing test) | 9 |
| signature (a parameter refinement rejects valid calls before any body runs) | 3 |
| type-definition (a struct refinement rejects values the tests build) | 1 |
| **total** | **13** |

The truth's spectrum rank (`cortex locate`) is 1 on 43 of the 44 in-scope
tasks and 2 on one. These tasks test generation, not localisation.

**Split.** Files that host an out-of-scope task go to `hard-dev`, with all
their in-scope tasks (13 files, 25 in-scope tasks). The remaining 12 files, in
sha256(path) order, each go to the set with fewer in-scope tasks, and all of
them landed in `hard-once`. The two sets share no source file. `hard-once` is
skewed toward documented-constant (13 of 19), because that is what those files
offered.

## Validity (zero-cost dry runs, `results/dryrun/`)

The dry runs used `run_ab.py` with the zero-cost agents, axon and cortex built
from `main` `f9ded716` (cortex `--features ai`), `CORTEX_AUDIT=off`, and no
OpenRouter key:

| run | hard-dev in-scope (25) | hard-dev out-of-scope (13) | hard-once (19) |
|---|---|---|---|
| B-oracle (`literal:` pristine body of `symbol`) | **25/25 verified**, 0 false successes | **0 claims, 13/13 honest failures** (exit 21 `no_progress`, file unchanged) | **19/19 verified**, 0 false successes |
| B-none | 0 verified, 0 claims (`needs_input`) | 0 claims, 13/13 unchanged | not run |
| A-oracle (splices the same one body, claims FIXED) | 25/25 verified | 0 verified, 13/13 false successes | not run |
| A-noop | 0 verified, 0 claims | 0 claims | not run |
| A-cheat (visible test -> `assert(true)`) | 0 verified, 25/25 false successes | 0 verified, 13/13 false successes | not run |

The A-oracle out-of-scope row is the point of those tasks. Even the correct
body for the named function, spliced and claimed, leaves the file failing. An
agent that stops after one function and says "fixed" is caught.

## Files

* `src/`: the 42 snapshots with their tests.
* `defects/{asi,core,misc}.json`: the planted specs.
* `build.py`: validation, real-bug replay, split, and writing both sets.
  `python3 build.py --check [defects/X.json ...]` validates without writing
  (default: every `defects/*.json`) and exits 1 if any spec FAILs; the full
  gate runs it with the built binaries.
* `selftest_check.py`: binary-free self-test of `build.py`'s exit status and
  arguments (`gate.sh` `stage_protected_sets`).
* `mining.json`: the real-bug replay report (every pair and reason).
* `rejected-ideas.json`: defect ideas dropped by their authors, with the reason.
* `hard-dev.json`, `hard-once.json`: the sets.
* `PREREG-hard-dev.md`: the first paid run on `hard-dev`.
* `results/dryrun/`: the zero-cost validity runs above.
* `results/hard-dev/`: the rows of paid runs.
