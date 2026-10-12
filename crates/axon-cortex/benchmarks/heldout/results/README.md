# Held-out results (PREREG-heldout.md)

Verdict per `../PREREG-heldout.md`: the ranker **PASSES** on heldout, narrowly.
0 false successes; verified 41 = 41 (McNemar p = 1.0); proposals per verified
fix 4.39 < 4.93. The gain is much smaller than on the dev set (3.55 -> 2.14).

Second finding, not part of the rule: **the plain micode agent verified 47/47
and beat both Cortex arms (41/47), paired exact McNemar p = 0.031.**

Prereg committed 862f1a34 (2026-10-12 18:52:40 -0400). First paid row: pilot
18:53:16. Cortex `--features ai` sha256 `895ec45d2b440804…` and axon
`60bfce385ce6e936…`, both built from 862f1a34. Generator/agent
`claude-haiku-5-5` via micode 0.3.0, `CORTEX_AUDIT=off`, breadth walk,
`--candidates 6`, budget 8, `--parallel 3` per arm, all three arms running at
the same time.

| full, n = 47 | B-spectrum | B-ranker | A plain |
|---|---|---|---|
| verified | 41 | 41 | **47** |
| false successes | 0 | 0 | 0 |
| proposals (all tasks) | 202 | 180 | — |
| proposals per verified fix | 4.93 | **4.39** | — |
| proposals on verified tasks only | 69 | 45 | — |
| generator model time, s (sum `api_ms`) | 625.6 | 529.7 | not recorded |
| model time on verified tasks, s | 178.8 | 90.4 | — |
| wall, s (sum per task) | 661.1 | 637.8 | 319.0 |
| est. generation cost | $0.0609 | $0.0526 | $0.0619 |
| ranker spend (OpenRouter) | — | $0.0216 | — |
| cost per verified fix (incl. ranker) | $0.00149 | $0.00181 | $0.00132 |
| true function attempted first | 25 | **45** | — |
| ranker fallbacks | — | 0 | — |

Paired verified results:
* B-ranker vs B-spectrum: both 41, 0 / 0 discordant, neither 6; p = 1.0.
* B-spectrum vs A: both 41, only A 6, only B-spectrum 0; p = 0.031. B-ranker
  vs A: the same 6 tasks.

## Reading it

* The ranker localizes well here too: truth first 45/47, against 25/47 for
  spectrum order. On verified tasks it roughly halves proposals (69 -> 45) and
  model time (179 s -> 90 s).
* Both Cortex arms fail the SAME 6 tasks: h-constant-03, h-constant-04,
  h-wrong-var-03, h-wrong-var-06, h-wrong-var-07 and h-wrong-field-04. In
  every one the true function was attempted, and in 5 of the 6 under the
  ranker it was attempted first. These are generation failures, not
  localization failures. They use about 133-135 proposals, three quarters of
  each arm's total, and that is why the overall gain in proposals per verified
  fix is small.
* The plain agent fixed all 6, so the bottleneck on this set is Cortex's
  generation step, not its ranking. [INFERENCE] One likely reason: the plain
  agent sees the visible tests and can iterate with tools, while Cortex's
  generator writes only one body per call.
* Cost per verified fix went up with the ranker ($0.00149 -> $0.00181) for the
  same reason as on dev: the Haiku 5.5 generator is so cheap that the ranker
  call costs about as much as it saves.

## Spend

Pilot (5 tasks x 3 arms): $0.0116. Full: $0.197. Total $0.209 of the $3 cap
(micode dollars are tokens × list price; ranker dollars are reported by
OpenRouter).

## Deviations

* The prereg did not say which 5 tasks the pilot uses. They were every 10th
  task (h-swap-branches-01/-02/-03/-06, h-wrong-var-04); all arms went 5/5
  with 0 false successes.
* Arm A rows have no `api_ms` (micode arm A logs tokens and turns), so its
  model time is not reported.
* `CORTEX_AUDIT=off` was also exported for arm A. It has no effect there.

## Files

`dryrun/run-B-oracle-spectrum.jsonl` (47/47 verified with this binary),
`pilot/{b-spectrum,b-ranker,a}.jsonl` (not analysed),
`full/{b-spectrum,b-ranker,a}.jsonl` plus `.log`. All three full runs are in
`../../scorecard/history.jsonl`.
