# Goal: Tune summary length, with the channel budget read from a file

Demonstrates the surface compiler **auto-including prelude helpers**: this
goal's `axon` block calls `length_score` and `normalize_score`, and
`axon-surface` pulls exactly those (and no others) into the generated `.ax`.
The scorer READS `./budget.txt`, which makes this goal's result depend on the
environment — so it is the fixture for `AXON_RECORD`/`AXON_REPLAY` over an
optimizer run (`scripts/replay_host_gate.sh` check 14). An optimizer's whole
output is score deltas, and a score delta is meaningless if the environment
moved underneath it; this is the goal that proves the deltas can be re-checked.
Runs end-to-end with no API key or feature.

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

- Reads `./budget.txt`; no LLM.

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

```axon
@[adaptive]
fn try_variant(x: i64) -> i64 {
    match read_file("./budget.txt") {
        Ok(s) => length_score(x, 180, 280)
        Err(e) => 0
    }
}
```
