# Goal: Improve across runs (persistent cross-run search)

Demonstrates **cross-run self-improvement**: each run's evaluation budget is too
small to reach the optimum from scratch, but `./run.sh improve` (which sets
`AXON_GOAL_CONTINUE=1`) resumes `goal_run`'s hill-climb from the best input seen
in the persisted provenance log — so the best score climbs run after run until
it converges. Pure objective → runs key-free.

```bash
cargo build -p axon-core --no-default-features --bin axon
# One command, autonomous iterate-to-converge (100 → 136 → 164 → 184 → 200):
XDG_CACHE_HOME=/tmp/learn ./target/debug/axon goal --iterate 6 examples/goals/learn-goal.md

# Or step it manually to see continuation resume from the best prior input:
XDG_CACHE_HOME=/tmp/learn ./target/debug/axon goal examples/goals/learn-goal.md   # fresh: ~modest score
XDG_CACHE_HOME=/tmp/learn AXON_GOAL_CONTINUE=1 ./target/debug/axon goal examples/goals/learn-goal.md  # resumes, higher
# (examples/asi/run.sh improve does the manual step for the asi demos.)
```

## Intent

Maximize an objective whose optimum is too far to reach within a single run's
budget, accumulating progress across runs via the provenance log.

## Inputs

- `x: i64` — the parameter to optimize.

## Outputs

- `best: i64` — the best objective score observed.

## Score (higher is better)

Objective `200 - (x - 12)^2`, peaking at `x = 12` with score `200`.

## Constraints (must hold)

- Pure function; no I/O, no LLM.

## Budget

- Up to 5 evaluations per run (deliberately too few to reach x = 12 from 0).

## Verify (post-conditions, gated at runtime)

```axon
@[verify(score >= 200)]
```

## Redteam (adversarial test cases)

- None — deterministic objective.

## Effect surface

- None. Pure compute.

## Provenance

- Each evaluation logged (with its input) to `~/.cache/axon/provenance.jsonl`;
  cross-run continuation reads it to resume the search.

## Implementation (author-supplied Axon bodies)

```axon
@[adaptive]
fn try_variant(x: i64) -> i64 {
    // Peaks at 200 (x = 12). One run's budget can't climb from 0 to 12;
    // AXON_GOAL_CONTINUE resumes from the best prior x.
    let d = x - 12
    200 - d * d
}
```
