# Goal: Tune a parameter to its optimum within a valid operating range

A self-contained constrained-optimization demo. Like `optimize-goal.md`, the
objective is a pure function (no API key / no `asi-runtime` needed), but here the
prose **Constraints** section is *enforced during the search*: the author supplies
a `feasible` predicate, so `axon goal` wires `goal_run_constrained` instead of the
plain `goal_run`. The optimizer only ever accepts candidates inside the feasible
region — a hard gate, not a soft penalty.

## Intent

Search an integer parameter `x` for the value that maximizes a scalar objective,
**subject to** `x` staying within the valid operating range `[1, 9]`. Candidates
outside the range are rejected by the optimizer, not merely penalized.

## Inputs

- `x: i64` — the parameter to tune.

## Outputs

- `best: i64` — the best objective score observed among feasible candidates.

## Score (higher is better)

The objective is `100 - (x - 5)^2`: a downward parabola peaking at `x = 5` with
score `100`. The optimum sits inside the feasible range, so the constrained
search converges on `x = 5`.

## Constraints (must hold)

- `1 <= x <= 9` — the valid operating range. Enforced during the search by the
  `feasible` predicate below; the optimizer never accepts an out-of-range `x`.
- The objective is a pure function: no I/O, no LLM, no network.

## Budget

- Up to 40 evaluations of the objective per run.

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

The author supplies both the `@[adaptive]` objective and a `feasible` predicate
over the same parameter. Because a `feasible` fn is present, the generated `main`
drives `goal_run_constrained("try_variant", "feasible", ...)`, so the prose
constraint is enforced, not just documented.

```axon
@[adaptive]
fn try_variant(x: i64) -> i64 {
    // Downward parabola, single optimum at x = 5 (score 100).
    100 - (x - 5) * (x - 5)
}

fn feasible(x: i64) -> bool {
    // The valid operating range [1, 9]. The optimizer rejects any x outside it.
    x >= 1 && x <= 9
}
```
