# PREREG-ranker: the Haiku 5.5 ranker on the dev set

Written and committed BEFORE any paid trial of this run. The git history of
this file and of `results-ranker/` is the evidence of that order. Anything
decided after the first paid trial is reported as a deviation in
`results-ranker/README.md`, never edited in here.

## Question

Does the model ranker `openrouter:anthropic/claude-haiku-5.5` (the
`cortex repair` default with a `--features ai` build) cut proposals, model
time and cost per verified fix on the 60 dev tasks, without losing verified
fixes or adding false successes, against the spectrum-order baseline?

**Caveat stated in advance.** The dev set (`corpus.json`) was used to build and
choose the ranker (LocRank, `benchmarks/locrank/`, 60/60 offline). A pass here
is NECESSARY, NOT SUFFICIENT: the held-back evaluation set decides whether the
ranker makes Cortex smarter. A fail here is a fail.

## Baseline (frozen, not re-run)

`results-micode/full/claude-haiku-5-5/b.jsonl` (PREREG-micode.md, arm B):
generator `claude-haiku-5-5` via `drivers/micode.sh`, breadth-first walk,
`--candidates 6`, default budget, cortex built WITHOUT `--features ai`, so
spectrum order. Recomputed from its rows: 56/60 verified, 0 false successes,
199 proposals, model time (sum of `api_ms`) 592.9 s, estimated generation
cost $0.0611, wall 633.3 s, truth first attempted 26/60.

## This run (one arm)

Identical to the baseline except the variable: `run_ab.py --arm B --generator
real --transport micode --model claude-haiku-5-5 --ranker
openrouter:anthropic/claude-haiku-5.5`, which sets `CORTEX_RANKER` to that
spec and `CORTEX_AUDIT=off` (so wall and model time contain no audit call).
Cortex and axon built from this branch (`cargo build -p axon-cortex --features
ai`, `cargo build -p axon-core --bin axon`), sha256 recorded in every row's
`meta`; `OPENROUTER_API_KEY` from the operator's env file. `--parallel 3`.
Same corpus (sha256 `3c7bc7bd…`), same grader, same claim rules.

A ranker call that fails falls back to spectrum order visibly (`ranker.used`
null, `ranker.reason` set). Such rows are kept and counted, and the count of
fallbacks is reported; they are not re-run.

## Sequence

1. Dry run, zero cost: `--generator oracle` with the ranker spec set to
   `spectrum` — must verify 60/60 (harness and build sanity).
2. Pilot: 5 tasks, ranker on, paid (`results-ranker/pilot/`). Purpose: check
   the ranker fields land in rows and measure per-task spend. Pilot rows are
   not part of the analysis.
3. Full: all 60 tasks (`results-ranker/full/b.jsonl`).

## Metrics (all recomputed from rows)

Per run: verified (`verified_clean`), false successes, proposals (sum of
`proposals`), proposals per verified fix, model time (sum of generator
`api_ms`) and per verified fix, estimated generation cost (tokens x list
price, `cost_usd`), ranker spend (sum of `ranker_cost_usd`, OpenRouter
reported), total cost per verified fix (both), wall time, truth_first (first
attempted function is the true defective one), ranker latency, fallbacks.

Paired on task id against the baseline: both/only-ranker/only-baseline/neither
verified, exact two-sided McNemar p on the discordant pairs.

## Decision rule

PASS iff all three:

1. 0 false successes;
2. verified not significantly lower: NOT (ranker verified < baseline verified
   AND McNemar exact two-sided p < 0.05);
3. proposals per verified fix strictly lower than the baseline's 199/56 = 3.55.

Otherwise FAIL. Reported, not decisive: model time and cost per verified fix,
truth_first, against the LocRank projection (proposals 199 -> ~76, model time
593 s -> ~306 s).

## Spend cap

$3.00 total across pilot and full run, counting OpenRouter ranker spend as
reported plus the micode generation estimate (`run_ab.spent` over
`results-ranker/`, `--cap-usd 3`). The micode estimate is not billed in
dollars (OAuth grant) and is reported as an estimate. The run stops launching
tasks at the cap; unfinished tasks are reported as missing.
