# Cortex Phase 5 — independent tasks: the method fails, and the confound stands

**Goal:** remove the confound I had flagged as most damaging — that I wrote the
tasks, the intents, the hidden tests, AND (earlier in this branch) several of
the compiler hints the treatment arm reads.

**Design:** real CPython stdlib source, mechanically mutated, with the REAL
function as a differential oracle on randomly generated inputs. None of the
code, the expected behaviour, or the test cases would be mine.

**Outcome: no measurement.** All 6 tasks that survived validation calibrate at a
control pass-rate of **1.00**. The model solves every one on the first attempt.
**The confound is NOT removed, and no Phase-5 number should be quoted.**

## Why: the model has memorised the stdlib

These are `statistics.median`, `email.utils.parseaddr`, `statistics.covariance`
and similar. The model does not need to find the injected defect — it rewrites
the function it already knows. A ceilinged control cannot measure a treatment,
which is the same wall Phase 1 and Phase 3b hit, reached by a different road.

This is **consistent with** the mechanism the earlier phases identified — the
gain appears where the model's prior is WRONG (Axon syntax it has never seen;
subtle Python edge cases) and there is nothing to correct where the prior is
already right. It does **not confirm** that mechanism: a ceilinged control
cannot discriminate between explanations.

## Three harness defects found on the way, each fatal if unnoticed

1. **Bare-namespace exec.** The oracle exec'd the extracted function with only a
   few imports, so it lost its module context — `shlex.split` needs the `shlex`
   class, `statistics.mean` needs `_sum`/`_convert`. Every control rate read
   0.00 and **13 of 13 tasks looked "hard"**. It was the harness failing to run
   the code at all. Fixed by exec'ing in a copy of the module's globals.

2. **No "the original passes" gate.** I checked that the MUTANT fails and never
   that the ORIGINAL passes — the exact gate Phases 1–2 had and this one
   dropped. It is what exposed defect 1, and it also removed tasks that are
   unfair by construction: `difflib.restore` returns a generator (compares by
   identity, never equal) and `email.utils.make_msgid` is nondeterministic.

3. **Silent truncation.** At a 650-token cap the model was cut off mid-function
   on long sources — `difflib.context_diff` is 75 lines — producing no closing
   fence. Those tasks would have calibrated as "hard" because the answer did not
   fit, not because the defect was subtle. Fixed by raising the budget to 1600
   and dropping sources over 32 lines.

Each of these produces a plausible-looking number. Only the validity gates
distinguished "the task is hard" from "the harness is broken".

## Standing

The Phase 2–4 pooled result (+23.5pp, 95% CI [+8.5, +40.5], p=0.018) is
unchanged by this. So is its principal weakness, which Phase 5 was built to fix
and did not:

> Every task in that estimate was authored by me, in a repo whose diagnostics I
> partly wrote.

**What would actually work:** a task source where the model's prior is wrong and
the code is not mine. Candidates: a real project's historical bug-fix commits
(pre-dating this session) with their own regression tests as the oracle; or an
unfamiliar third-party library rather than the stdlib. Stdlib mutants are
disqualified for this model — memorisation puts the control at the ceiling.
