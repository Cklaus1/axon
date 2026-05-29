# Goal: Deploy a tuned parameter only if it clears the safety bar

A self-contained demo of Axon's core ASI safety property: a **verified deploy
gate that refuses to ship an under-performing agent**. The objective here can
only reach 60/100, but the deploy gate demands confidence ≥ 0.9 — so `axon goal`
hill-climbs to the best achievable value and then *blocks deployment* (the
`@[verify]` gate panics). Runs end-to-end with no API key.

## Intent

Tune a parameter `x` to maximize an objective, then gate deployment on a
confidence floor. The objective is deliberately capped below the floor to show
the gate firing.

## Inputs

- `x: i64` — the parameter to optimize.

## Outputs

- `best: i64` — the best objective score observed.

## Score (higher is better)

Objective `60 - (x - 3)^2`, peaking at `x = 3` with score `60`.

## Constraints (must hold)

- Pure function: no I/O, no LLM, no network.

## Budget

- Up to 20 evaluations per run.

## Verify (post-conditions, gated at runtime)

```axon
@[verify(confidence >= 0.9)]
```

Deployment requires confidence ≥ 0.9 (i.e. a best score ≥ 90 on the 0–100
scale). The objective caps at 60, so the gate must block.

## Redteam (adversarial test cases)

- None — deterministic objective.

## Effect surface

- None. Pure compute.

## Provenance

- Every evaluation logged to `~/.cache/axon/provenance.jsonl`.

## Implementation (author-supplied Axon bodies)

The author supplies both the `@[adaptive]` objective and an **enforced** deploy
gate. `assert_deployable` returns an `Uncertain<i64>` whose confidence is the
best score on a 0–100 scale; the `@[verify(confidence >= 0.9)]` clause is a
runtime gate (Axon's confidence-lattice form), so it panics — blocking the
deploy — when the achievable best falls short.

```axon
@[adaptive]
fn try_variant(x: i64) -> i64 {
    // Objective peaks at 60 (x = 3) — deliberately below the 0.9 safety bar.
    let d = x - 3
    60 - d * d
}

@[verify(confidence >= 0.9)]
fn assert_deployable(score: i64) -> Uncertain<i64> {
    // Confidence = best score on a 0..100 scale. Below 0.9 → gate blocks deploy.
    uncertain_dyn_i64(score, i64_to_f64(score) / 100.0)
}
```
