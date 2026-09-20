# Localization accuracy, measured

`cortex locate` ranks the functions a defect might be in. These scripts measure
how often it is right, against real Axon code rather than a fixture written to
be repairable.

```bash
cargo build -p axon-core --bin axon && cargo build -p axon-cortex --bins
python3 benchmarks/topk.py ../../examples/stdlib/*.ax ../../examples/*.ax
```

## Method, and what it does not cover

A defect is injected into one function at a time, and the mutant is KEPT only
if it still compiles AND at least two checks now fail. One failing check
becomes the ADJUDICATOR — it must fail, or it cannot witness a repair — and the
rest remain as the visible evidence localization works from.

Five defect shapes, because an earlier version of this file measured one and
reported the result as though it were general: an operator swap, a bumped
constant, a flipped boolean, two swapped arguments, and a deleted statement. A
wrong constant carries no operator to spot; swapped arguments leave every token
in place; a deleted statement removes code rather than altering it.

Being liberal about mutators is safe by construction — a mutant that fails to
compile, or that no check notices, is discarded — so a mutator that usually
produces nonsense costs sample size and never costs correctness.

Scoring is four-way rather than pass/fail. A correct name, a wrong name, an
honest refusal and an error have four different remedies, and collapsing
refusals into failures would make a system that knows what it does not know
look identical to one that guesses.

### Running it

```bash
cargo build -p axon-core --bin axon && cargo build -p axon-cortex --bins
python3 topk.py      ../../examples/stdlib/*.ax ../../examples/*.ax   # localization
python3 oracle.py    ../../examples/stdlib/*.ax ../../examples/*.ax   # upper bound
python3 adversary.py ../../examples/stdlib/*.ax ../../examples/*.ax   # safety floor
python3 guessing.py  ../../examples/stdlib/arrays.ax                  # a real generator
```

## Results (2026-09-20, 111 trials, five defect classes)

29 files, 81 distinct functions. One defect injected per trial and kept only if
it still compiles and at least two checks fail — one becomes the adjudicator
(it must FAIL, or it cannot witness a repair) and the rest stay as visible
evidence.

### Localization

| class | n | top-1 (sole candidate) | top-3 | truth absent |
|---|---|---|---|---|
| operator | 43 | 41.9% | 93.0% | 0.0% |
| constant | 47 | 27.7% | 83.0% | 0.0% |
| boolean | 12 | 25.0% | 91.7% | 0.0% |
| argswap | 6 | 16.7% | 100% | 0.0% |
| drop-statement | 3 | 0.0% | 100% | 0.0% |
| **all** | **111** | **31.5%** | **89.2%** | **0.0%** |

Top-3 holds across classes; top-1 does not. A wrong constant gives the spectrum
far less to separate candidates with than a wrong operator, which is the
argument for walking the ranking rather than trusting its head.

**Read "truth absent 0.0%" with care** — it is close to guaranteed by
construction, since the defect is injected into a function some failing check
reaches. The informative numbers are top-3 and the narrowing: the ranking is a
median 6 candidates out of a median 11 functions that could have been blamed,
and covers the whole universe in only 9 of 111 trials.

### The loop, with a perfect generator

The generator is handed the body the file had before it was broken, so it
cannot propose anything wrong. Every remaining failure is the control loop's.

| | |
|---|---|
| verified, and the file really is clean | **99 / 111 (89.2%)** |
| verified but the file still fails | **0** |
| repaired the function that was actually broken | 99 / 99 |
| failed, workspace restored byte-for-byte | 12 / 12 |

**The 12 failures are exactly the 12 trials where the broken function falls
outside the top-3 ranking — the same set, not merely the same count.** With a
perfect generator the loop repairs precisely what localization finds and loses
nothing of its own, so the repair rate is bounded by the ranking and by nothing
else in the machinery.

### The loop, with a confidently WRONG generator

The generator proposes something plausible, compiling and incorrect — what a
model produces when it has misunderstood the defect.

| | |
|---|---|
| false successes (exit 0, file still fails) | **0 / 68** |
| workspace unchanged after a failed run | 65 / 65 completed runs |

The one run that left the workspace altered was killed by the harness's own
timeout before it could restore.

Earlier versions of this table reported 4 false successes in 68. Each was a
patch to the WRONG function that the single hidden check happened to accept
while other checks still failed. Verified now means no check in the file is
failing: the adjudicator is a witness, not the whole truth.

### The loop, with a generator that GUESSES

`guess.py` proposes plausible single-token edits in a fixed order. It knows
nothing about which defect was injected and must learn from the rejections the
loop feeds back. It is not a model, and its numbers are not a model's — what it
measures is whether the loop can be DRIVEN to a repair by something that
guesses, which is the shape of the problem a model faces minus the
understanding.

| | |
|---|---|
| repaired, file clean | 8 / 15 (53.3%) |
| false successes | 0 |
| ran out of ideas and said so (exit 24) | 4 |
| exhausted the step budget (exit 20) | 2 |

It is also the first real consumer of the rejection feedback, and building it
found the defect that made that feedback useless: the loop kept rejected
patches in the file, so each proposal replaced code the previous proposal had
broken.

### Exit 27 — adjudicated, but not clean

A run can satisfy its adjudicator, break nothing, and still leave the file
failing checks that were ALREADY failing when it started. That is not success
and it is not futility:

* it is not exit 0, because a file that fails checks is not repaired;
* it is not exit 21, because the generator did what it was asked — reporting
  "going in circles" blamed a generator that had succeeded;
* and the patch is KEPT rather than rolled back, because discarding it would
  throw away work that answered the only question the run was given.

Earlier the loop did all three wrong things at once: it undid the working
patch, recorded the correct body as "did not fix the problem", and put it on
the generator's do-not-propose list.

### At the EDGE of what the loop can express

Every class above is a defect the loop CAN repair — one wrong thing inside one
function body, the exact shape of its single action. So none of them said
anything about the boundary, which is the part that decides whether a
controller can be trusted with more.

This class swaps the bodies of two same-signature functions. It compiles, the
file's own checks catch it, and no single `PatchSymbolBody` can undo it:
repairing either half leaves the other wrong. The generator is handed the
correct body for one of them — the best a real generator could ever do here.

| | |
|---|---|
| trials | 16 |
| exit 0 | **0** |
| false successes | **0** |
| workspace unchanged | 16 / 16 |

All sixteen ended as `no_progress`. Outside what its action can express, the
loop does not claim success and does not alter the workspace.

Two things this does NOT show. It says nothing about how OFTEN real defects
fall outside the capability — the class is synthetic and chosen, so its
frequency here is a property of the mutator. And the premise had to be checked
per trial rather than assumed: the first version asserted "a swap needs two
edits" from the construction, and one swap turned out to be repairable by a
single patch because the borrowed body still satisfied everything that
exercised it. That row would have sat in these results as a safety datum about
a case that does not belong here.

## Corrections, recorded

Four, because a measurement that was wrong and then quietly restated is worse
than no measurement.

**The adjudicator was chosen before the defect was known.** It was the file's
last `@[test]`, which usually did not exercise the injected defect — so it
PASSED on the broken file, and 54 of 80 runs reported `verified_done`, most at
step 1, having changed nothing. The harness now picks an adjudicator from the
checks that actually fail, and `cortex repair` refuses one that already passes
(exit 26). Those figures are withdrawn, not restated: the trial set is not
comparable.

**The published top-1 counted alphabetical tie-breaks as hits.** When several
candidates tie, the ranking orders them by name — an accident of spelling. The
figure reported here is the SOLE most-suspicious candidate; the per-trial rows
carry `tie_at_top` so the distinction stays checkable.

**The harness had the same fail-open the code did.** `failing_tests` returned
`[]` for a run that never completed, and `file_clean` — the independent
evidence behind every "0 false successes" — read that as a clean file. Fixed,
and both headline results were re-measured afterwards and survive it.

**`measure.py` was committed dead.** It grepped stdout for a message a later
commit had renamed, and invoked `cortex repair` with no grant and no
generator, so every trial was refused and scored "error". It could not have
produced the numbers printed beside it. Deleted; `topk.py`, `oracle.py`,
`adversary.py` and `guessing.py` read `--json`.

## Call depth

`MAX_CALL_DEPTH` decides how far the spectrum follows calls from a check.

| depth | top-3 | truth absent |
|---|---|---|
| 1 (direct calls only) | 80.6% | 2.8% |
| 2 | 90.1% | 0.9% |
| **4 (shipped)** | 89.2% | **0.0%** |

Following calls at all is worth about ten points. Depth 2 and 4 differ by one
trial in each direction, which is noise — an earlier version of this table read
differences that size as a result, and quoted a measurement this file had
already withdrawn. Four is kept for the one thing that separates them: it is
the smallest depth at which the broken function was never ABSENT from the
ranking, and a candidate that is never listed cannot be tried.

## Stated limitations

* **All defects are injected, and I chose them.** An attempt to build a corpus
  from real bug-fix commits in this repository produced zero usable trials —
  its `.ax` history is examples being added and refactored, not behavioural
  bugs caught by in-file tests. The funnel is recorded in the session notes;
  the honest summary is that no real-defect validation exists here.
* **Every defect lives inside one function body**, because that is the only
  edit the loop can make. Nothing here says how often real defects have that
  shape.
* **A file with a failing check unrelated to the defect can never verify.**
  Requiring no check to fail is what closed the false-success class, and the
  loop cannot tell which failures it was responsible for. Claiming success
  while a check fails is the defect that rule exists to prevent, so the
  conservative direction is deliberate.
* **The numbers are not a model's.** Both generators here are deterministic.
  No model has ever driven this loop: this environment has no credentials.
