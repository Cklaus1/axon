# Generator-context results (PREREG-gencontext.md)

Verdict per `../../PREREG-gencontext.md`: **Cortex matches the plain agent on
`once.json`**, both B (default config, spectrum order) and B' (Haiku 5.5
ranker). 0 false successes in every arm; verified 30 = 30 = 30, 0 discordant
pairs, exact McNemar p = 1.0; A - B = 0 <= 2.

Prereg committed 74dbc4e3 (2026-10-12 19:28:40 -0400), alone. Change under
test ef58330b. Rows record commit e9bc5d0a (ef58330b + the prereg + the
`generator_context` row field in `run_ab.py`). First paid row: the
exploratory heldout run, after e9bc5d0a; rows committed in 02968728. Cortex
`--features ai` sha256 `b7cc897251b2484a…`, axon `--no-default-features`
`0fc0215fd564e37b…` (16-hex prefixes in each row's `meta`). Generator/agent
`claude-haiku-5-5` via micode 0.3.0, `CORTEX_AUDIT=off`, breadth walk,
`--candidates 6`, budget 8; the three once arms ran at the same time,
`--parallel 1` each.

## Primary: once.json, n = 30

| | A plain | B spectrum | B' ranker |
|---|---|---|---|
| verified | 30 | 30 | 30 |
| false successes | 0 | 0 | 0 |
| proposals (all tasks) | - | 34 | 30 |
| proposals per verified fix | - | 1.13 | **1.00** |
| wall, s (sum per task) | 189.6 | **76.4** | 91.4 |
| generator model time, s (sum `api_ms`) | not recorded | 67.6 | 42.9 |
| est. generation cost | $0.0344 | $0.0156 | $0.0122 |
| ranker spend (OpenRouter) | - | - | $0.0099 |
| cost per verified fix (incl. ranker) | $0.00115 | $0.00052 | $0.00074 |
| true function attempted first | - | 22 | 30 |
| generator-context fallbacks (body-only) | - | 0 | 0 |

Paired verified: B vs A both 30, 0 / 0 discordant, p = 1.0; B' vs A the same.

## Exploratory: heldout.json, B spectrum, n = 47

Labelled exploratory: the set and two of its transcripts were inspected to
diagnose the old failure. Same config as the old B-spectrum row, only the
prompt changed.

| | old B spectrum (`../full/`) | new B spectrum |
|---|---|---|
| verified | 41 | **47** |
| false successes | 0 | 0 |
| proposals (all tasks) | 202 | 93 |
| proposals per verified fix | 4.93 | 1.98 |
| wall, s | 661.1 | 299.3 |
| generator model time, s | 625.6 | 276.4 |
| est. generation cost | $0.0609 | $0.0620 |

All six old misses (h-constant-03/-04, h-wrong-var-03/-06/-07,
h-wrong-field-04) verified. The plain agent's 47/47 on the same set is in
`../full/a.jsonl`.

## Spend

$0.134 of the $2 cap (`run_ab.spent` over this directory): heldout B $0.062,
once A $0.034, B $0.016, B' $0.012 + $0.010 ranker. Micode dollars are tokens
x list price; ranker dollars are reported by OpenRouter.

## Deviations

* `run_ab.py` gained the `generator_context` row field (e9bc5d0a) after the
  prereg commit and before any paid row. The prereg's fallback metric reads
  it; nothing else in the harness changed.
* B' reports a ranker `reason` on 4 tasks (`one eligible candidate: nothing to
  reorder`). Those are not fallbacks (no model call needed); 0 real
  fallbacks.
* Arm A rows have no `api_ms` (micode arm A logs tokens and turns).
* The once set was run before this candidate won on heldout (`../../README.md`
  rule 4); stated in the prereg, as the operator's choice.

## Files

`dryrun/run-B-oracle-spectrum.jsonl` (30/30 verified, 0 false successes, zero
cost, these binaries), `heldout-exploratory/b-spectrum.jsonl`,
`once/{a,b-spectrum,b-ranker}.jsonl`, each with its `.log`.
`smoke/`: the first prompt the generator got for the true function in
h-wrong-var-03 and h-constant-03, captured offline (no model) by a driver that
saves stdin. All four paid runs are in `../../../scorecard/history.jsonl`.
