# Goal: Find the input that maximizes a deterministic objective

A self-contained goal-directed optimization demo. Unlike `hello-goal.md` (which
calls an LLM), this goal's objective is a pure function, so `axon goal` runs it
end-to-end with **no API key and no `asi-runtime` feature** — it exercises the
core ASI loop (`goal_run` hill-climbing an `@[adaptive]` function) in isolation.

## Intent

Search an integer parameter `x` for the value that maximizes a scalar objective.
The objective has a single optimum; the goal loop should discover it.

## Inputs

- `x: i64` — the parameter to optimize.

## Outputs

- `best: i64` — the best objective score observed.

## Score (higher is better)

The objective is `100 - (x - 7)^2`: a downward parabola peaking at `x = 7` with
score `100`. The optimizer should converge on `x = 7`.

## Constraints (must hold)

- The objective is a pure function: no I/O, no LLM, no network.

## Budget

- Up to 20 evaluations of the objective per run.

## Verify (post-conditions, gated at runtime)

```axon
@[verify(score >= 100)]
```

## Redteam (adversarial test cases)

- None — the objective is deterministic, so there is no adversarial input.

## Effect surface

- None. Pure compute; no capabilities required.

## Provenance

- Every objective evaluation is logged to `~/.cache/axon/provenance.jsonl`.

## Implementation (author-supplied Axon bodies)

The author supplies the `@[adaptive]` objective directly, so the compiler uses
it as the goal-loop target instead of the default LLM-driven prompt search.

```axon
@[adaptive]
fn try_variant(x: i64) -> i64 {
    // Downward parabola, single optimum at x = 7 (score 100).
    // goal_run hill-climbs x toward the target score.
    let d = x - 7
    100 - d * d
}
```
