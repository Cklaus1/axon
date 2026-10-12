# PREREG-heldout: does the ranker hold its dev-set gains on held-out tasks?

Written and committed BEFORE any paid trial on `heldout.json`. The git history
of this file and of `results/` is the evidence of that order. Anything decided
after the first paid trial is a deviation, reported in `results/README.md`,
never edited in here.

## Question

On the dev set (`../ab/PREREG-ranker.md`, `../ab/results-ranker/`), the Haiku
5.5 ranker `openrouter:anthropic/claude-haiku-5.5` kept verified fixes (56/60
vs 56/60), added no false successes and cut proposals per verified fix 3.55 ->
2.14. The dev set was used to build the ranker, so that pass was necessary,
not sufficient. Does the ranker hold those gains on 47 tasks it was never
tuned on?

## Set

`heldout.json`, 47 planted tasks, sha256
`2b977f7d5cbede6fed3ce59d85b871f9cd6a13b2477fe3379e74a82b4987d3fa`, frozen
(see `README.md`). `once.json` is NOT run.

**Less room to gain, stated in advance:** 25/47 heldout tasks already have the
truth at spectrum rank 1 (dev: 26/60), so the ranker can reorder usefully in at
most 22 tasks here (dev: 34).

## Arms (same model, same binaries)

Generator / agent `claude-haiku-5-5` via micode 0.3.0 (`ANTHROPIC_BASE_URL`
unset), `CORTEX_AUDIT=off`. Cortex built `--features ai` and axon built from
this branch (sha256 recorded in each row's `meta`); one cortex binary for both
B arms. Breadth-first walk, `--candidates 6`, default budget 8.

| arm | command (`../ab/run_ab.py heldout.json ...`) | rows |
|---|---|---|
| B-spectrum | `--arm B --generator real --transport micode --model claude-haiku-5-5 --ranker spectrum` | `results/full/b-spectrum.jsonl` |
| B-ranker | same, `--ranker openrouter:anthropic/claude-haiku-5.5` | `results/full/b-ranker.jsonl` |
| A plain agent | `--arm A --agent real --transport micode --model claude-haiku-5-5` | `results/full/a.jsonl` |

`--parallel 3` per arm; the arms may run concurrently (at most 9 model calls
in flight). A ranker call that fails falls back to spectrum order visibly;
such rows are kept and counted, not re-run.

## Sequence

1. Dry run, zero cost: B `--generator oracle` with `--ranker spectrum`; must
   verify 47/47.
2. Pilot: 5 tasks per arm (`results/pilot/`), paid; checks fields and spend.
   Not analysed.
3. Full: 47 tasks per arm.

## Metrics (recomputed from rows by `../scorecard/scorecard.py`)

Per arm: verified (`verified_clean`), false successes, proposals per verified
fix (B), generator model time (sum `api_ms`), estimated cost (micode tokens x
list price) plus OpenRouter-reported ranker spend, truth_first (B: first
attempted function is the true one). Paired on task id: both / only one /
neither, exact two-sided McNemar on verified.

## Decision rule (the dev rule, unchanged)

The ranker PASSES on heldout iff all three hold for B-ranker vs B-spectrum:

1. 0 false successes in B-ranker;
2. verified not significantly lower: NOT (B-ranker verified < B-spectrum
   verified AND exact two-sided McNemar p < 0.05);
3. proposals per verified fix strictly lower than B-spectrum's.

Otherwise FAIL. Reported, not decisive: model time, cost per verified fix,
truth_first, and B-spectrum vs A (verified, false successes, McNemar).

## Spend cap

$3.00 total across all pilots and full runs of all arms: `run_ab.spent` over
`results/` (`--spend-root results --cap-usd 3`) counts micode generation
estimates plus OpenRouter ranker spend. Micode dollars are estimates (OAuth,
no bill). Tasks not started at the cap are reported missing.
