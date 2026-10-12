# results-ranker: the Haiku 5.5 ranker on the dev set (PREREG-ranker.md)

Verdict per `../PREREG-ranker.md`: **PASS** — necessary, not sufficient. The dev
set was used to build the ranker (LocRank); the held-back set decides.

Prereg committed 14194529 (2026-10-12 18:05:52 -0400) before the first paid row.
Arm B only: `cortex repair` with generator `claude-haiku-5-5` via
`drivers/micode.sh` (micode 0.3.0), breadth-first walk, `--candidates 6`,
`CORTEX_RANKER=openrouter:anthropic/claude-haiku-5.5`, `CORTEX_AUDIT=off`,
`--parallel 3`. Cortex built `--features ai` from 14194529
(sha256 `73c02176a800b273…`), axon `1ba97e922586984f…`.

Baseline: `../results-micode/full/claude-haiku-5-5/b.jsonl` (same model and
transport, spectrum order). Reproduce: `python3 analyse_ranker.py`.

| | baseline (spectrum) | ranker | LocRank projection |
|---|---|---|---|
| verified / 60 | 56 | 56 | |
| false successes | 0 | 0 | |
| proposals (all tasks) | 199 | **120** | ~76 |
| proposals per verified fix | 3.55 | **2.14** | |
| generator model time, s (sum `api_ms`) | 592.9 | **332.8** | ~306 |
| model time per verified fix, s | 10.59 | 5.94 | |
| wall, s (sum per task) | 633.3 | 488.9 | |
| truth first attempted | 26 | **59** | |
| est. generation cost (list price) | $0.0611 | $0.0334 | |
| ranker spend (OpenRouter reported) | — | $0.0331 | |
| total cost per verified fix | $0.00109 | $0.00119 | |
| ranker fallbacks / mean latency | — | 0 / 2106 ms | |

Paired verified (n = 60): both 54, only ranker 2 (argswap-05, drop-stmt-05),
only baseline 2 (operator-03, boolean-08), neither 2 (argswap-01,
drop-stmt-02); exact McNemar p = 1.0.

Criteria: 0 false successes ✓; verified not significantly lower ✓ (equal);
proposals per verified 2.14 < 3.55 ✓.

## Reading it

* The ranker put the true function first in 59/60 tasks (spectrum 26/60), as
  LocRank predicted offline (60/60).
* Proposals fell 40%, not the projected 62%: the projection counted
  localization savings only. Failed tasks still burn their budget (37 of the
  120 proposals are on the 4 unverified tasks), and the discordant failures are
  generation failures on the RIGHT function (operator-03: 4 proposals on
  `score`, the truth, first; boolean-08: truth first, 14 proposals) — the
  generator is stochastic and the baseline's single-shot fixes there did not
  repeat. On verified tasks alone proposals went 151 → 83.
* Model time 332.8 s is close to the ~306 s projection. Wall fell less than
  model time because each task adds one ranker call (~2.1 s).
* Cost per verified fix is NOT lower: the Haiku 5.5 generator is so cheap
  ($0.0006 per fix saved) that the ranker's own call (~$0.00055 per task)
  cancels it. Cost was reported, not a decision criterion.

## Spend

Pilot (5 tasks, `pilot/`): $0.0114. Full: $0.0665 (generation estimate
$0.0334 + OpenRouter ranker $0.0331). Total $0.078 of the $3 cap. Micode
generation dollars are tokens × list price (OAuth grant, no bill).

## Deviations

None from the prereg. The dry run (`dryrun/b-oracle.jsonl`, oracle generator,
ranker `spectrum`) verified 60/60, 0 false successes.

## Files

`dryrun/b-oracle.jsonl`, `pilot/b.jsonl` (not analysed), `full/b.jsonl` (60
rows; `ranker`, `audit`, `escalation`, `ranker_cost_usd` per row),
`analyse_ranker.py`.
