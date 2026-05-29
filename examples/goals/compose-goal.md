# Goal: Tune summary length using shared scoring helpers

Demonstrates the surface compiler **auto-including prelude helpers**: this
goal's `axon` block calls `length_score` and `normalize_score`, and
`axon-surface` pulls exactly those (and no others) into the generated `.ax`.
Pure objective → runs end-to-end with no API key or feature.

## Intent

Find the summary length that best fits the channel, scoring with the shared
`length_score` helper rather than hand-rolled arithmetic.

## Inputs

- `x: i64` — candidate summary length in characters.

## Outputs

- `best: i64` — the best length score observed.

## Score (higher is better)

`length_score(x, 180, 280)`: full marks at ≤180 chars, decaying to 0 at 280,
normalized to 0..100.

## Constraints (must hold)

- Pure function; no I/O, no LLM.

## Budget

- Up to 20 evaluations per run.

## Verify (post-conditions, gated at runtime)

```axon
@[verify(score >= 100)]
```

## Redteam (adversarial test cases)

- None — deterministic objective.

## Effect surface

- None. Pure compute.

## Provenance

- Every evaluation logged to `~/.cache/axon/provenance.jsonl`.

## Implementation (author-supplied Axon bodies)

The objective calls `length_score` and `normalize_score` from the goal prelude;
the compiler auto-includes them (and omits the prelude helpers this goal does
not reference).

```axon
@[adaptive]
fn try_variant(x: i64) -> i64 {
    let raw = length_score(x, 180, 280)
    normalize_score(raw, 100)
}
```
