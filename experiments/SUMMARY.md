# Cortex experimental programme — consolidated standing

Five phases, ~1,000 episodes, all on local models. What is established, what is
not, and what would change the answer.

## The claim, with its evidence

**Grounded iteration beats blind iteration at equal attempt budget.**

Pooled over 19 task-level paired deltas from 3 task sets and 2 languages:
**+23.5pp, bootstrap 95% CI [+8.5, +40.5], 12 of 15 changed tasks improved,
sign test p = 0.0176.**

| phase | task set | control | treatment | delta |
|---|---|---|---|---|
| 2 | Axon syntax (7 tasks) | 51% | 78% | +26.1pp |
| 3a | Axon syntax, 4-arm ablation (7) | 56% | 79% | +22.9pp |
| 4 | Python, calibrated (5 tasks) | 59% | 80% | +21.0pp |

The arms differ in ONE thing: whether attempt N+1 sees the compiler/interpreter's
real diagnostic on attempt N. Attempt count, model, tasks, tools, decoding and
token limit are identical, so this is information, not compute.

## What the mechanism is

Phase 3a's four-arm ablation separates it:

| arm | repaired | vs blind |
|---|---|---|
| blind — same prompt again | 56% | — |
| **prev** — its own previous attempt, no diagnostic | 50% | **−5.7pp** |
| **diag** — the diagnostic, without the attempt | 64% | **+8.6pp** |
| both | 79% | +22.9pp |

`prev` is the control that carries the argument: same shape and length of extra
text, no new information — and it does not help. **So the effect is not "more
context".** The diagnostic carries the information, and it needs the attempt
beside it to be actionable (+8.6 alone vs +22.9 together). Sharpest case:
`dict_method_syntax` is 0% for blind, prev AND diag, and 100% for both.

The gain concentrates where the model holds a CONFIDENT WRONG PRIOR and the
diagnostic contradicts it by name. `invalid_program` failures fell 49 → 20.

## What is NOT established

1. **The task-authorship confound stands.** Every task in the pooled estimate is
   one I wrote, in a repo whose diagnostics I partly wrote earlier in this same
   branch. **Phase 5 was built to remove this and failed** — stdlib mutants
   calibrate at a control of 1.00 because the model has memorised them, so the
   experiment produced no measurement at all.
2. **Two models, one repo.** Phases 1–4 are Qwen2.5-Coder-7B; Phase 5 used a
   served Qwen3.8-Flash-Next but produced no usable data.
3. **Known harm.** Three tasks got WORSE under the treatment
   (`enum_variant_called`, `string_interp`, `wildcard_match` −20pp). Phase 1's
   `count_skips_last` shows the mechanism: added deliberation displacing a
   simple correct edit. This is a real cost, not noise.

## The methodological finding, which may outlast the result

**Every phase was limited by task-set validity, never by compute.** 1,000
episodes cost minutes; the binding constraints were always (a) is the control
off the ceiling, and (b) does the task measure what it claims.

Gates that each caught a fatal problem BEFORE it became a number:

- *broken must fail, reference fix must pass* — rejected 2/12 Phase-1 tasks
  whose hidden tests the BROKEN program already passed.
- *the task must actually be broken* — rejected 6/13 Phase-2 candidates (Axon
  permits reassignment without `mut`; it has `for x in xs`).
- *the original must pass its own oracle* — the gate I OMITTED in Phase 5, whose
  absence made 13/13 tasks read as "hard" when the harness simply could not run
  the code.
- *control must be off the ceiling* — killed Phase 3b entirely (100% both arms)
  and 11/12 Phase-1 candidates.

And one error in my own method: I first floored the admission band at a 0.15
control rate, which would have discarded the single most informative task shape.
**A 0% control has maximum headroom** — Phase 2's largest effect was exactly
such a task (0% → 87%). Only a ceilinged control is useless.

## What would change the answer

An independent task corpus: real historical bug-fix commits with their own
regression tests, or an unfamiliar third-party library — code, expected
behaviour and tests none of them mine. Surveyed for this: the local stdlib is
disqualified (memorised), and the third-party projects on this machine yield
~6 pure functions, each needing a bespoke input generator. **Building an
adequate corpus is a project, not a next step** — and until it exists, the
pooled +23.5pp should be read as a strong within-repo result, not a general one.
