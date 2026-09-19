# Cortex Phase 1 — harness validation

**Milestone goal, as set:** produce a *trustworthy measurement* of Cortex
behaviour under a fixed protocol. Not "prove Cortex works".

**Budgets:** 50 episodes or 120 min, whichever came first.
**Consumed:** 50 episodes, **0.77 min** wall-clock. The episode budget bound;
the wall-clock budget was never close.

**Setup:** `Qwen/Qwen2.5-Coder-7B-Instruct`, bfloat16, no quantization, local,
greedy decoding, 320 max new tokens, 0 retries, single GPU. 10 repair tasks over
the `axon` repo at commit recorded per episode. Paired: every task runs in both
arms back to back, so a wall-clock stop cannot leave one arm holding the easier
tasks.

**Arms.** Identical model, weights, tasks, tools, decoding, token budget and
retry count. The single difference is what the model is shown:

- `OFF` — the broken program and the intent.
- `ON` — the same, plus grounded observation (real `axon check` output, real
  failing-test output) and notice that an independent hidden check will judge
  the patch.

Both arms pay the same tool cost (4 calls/episode); only `ON` is *shown* the
result, so the arms differ in information and not in compute.

## Acceptance criteria: instrumentation integrity — PASS (8/8)

| check | result |
|---|---|
| episodes recorded == episodes reported | PASS |
| arms balanced (25/25) | PASS |
| every episode evaluable | PASS |
| every precondition held (task really was broken) | PASS |
| all 19 required telemetry fields present on every record | PASS |
| tasks paired across arms | PASS |
| arms actually differ at the input (prompt length) | PASS |
| same seed ⇒ same patch (reproducible) | PASS |

The task set was itself validated before use: each task must have its broken
form FAIL both visible and hidden checks and its reference fix PASS both. **Two
of twelve candidate tasks were rejected** by that gate — their hidden tests were
passed by the *broken* program, so they could not have distinguished a repair
from no repair. They were repaired and re-admitted.

## Outcomes — reported, but NOT a claim about Cortex

| arm | n | repaired | tokens/ep | wall/ep | tools/ep |
|---|---|---|---|---|---|
| OFF | 25 | 25 (100%) | 44 | 0.93 s | 4.0 |
| ON | 25 | 22 (88%) | 44 | 0.92 s | 4.0 |

**Three reasons this cannot support a claim of improvement, in either
direction:**

1. **The effective sample size is 10, not 50.** Greedy decoding ignores the
   seed, so repeated episodes of the same (task, arm) are the *same run*:
   19 of 20 groups produced a byte-identical patch across different seeds. The
   50 episodes are 10 independent observations with 2–3× duplication.

2. **The baseline is at the ceiling.** OFF repaired 100%. A task set the
   control already solves completely has no headroom to show improvement — it
   can only show harm. This measures the task set, not the treatment.

3. **20 of 25 pairs produced a byte-identical patch.** On most of these tasks
   the added context changes nothing at all, because a one-line fix under greedy
   decoding converges regardless of framing.

## The one real difference, and it went the wrong way

`count_skips_last` — OFF 3/3, ON 0/3, consistently.

OFF produced the minimal correct fix (`i = i + 1`). ON produced:

```axon
let c = 0;
let i = 0;
let mut c = c;      // redundant restructuring
let mut i = i;
while i < n {
    c = c + 1;
    i = i + 2;      // the ACTUAL defect, left in place
}
```

The extra context induced restructuring of the bindings and the model never
made the one-line change the task needed. This is the predicted failure mode of
a control layer: added deliberation displacing the simple correct action.

n=1 task. It is a lead, not a result.

## What Phase 2 must change

- **Break the greedy-duplicate problem.** Either sample at temperature with
  recorded seeds, or use distinct task instances per episode. Repeats that
  reproduce the same bytes are not samples.
- **Raise task difficulty until the baseline is off the ceiling.** Target a
  control pass-rate near 40–70%, or the design cannot detect improvement.
- **Keep the 4-call tool parity**, and add a token/compute column that separates
  prompt from completion cost, since the ON prompt is ~2.8× longer and that is a
  real cost even when the answer is identical.
