# PREREG-hard-dev: is the hard set hard, and does Cortex fail honestly out of scope?

This file was written and committed alone, BEFORE any paid trial on
`hard-dev.json`. The git history of this file and of `results/hard-dev/` is
the evidence of that order. Anything decided after the first paid trial is a
deviation. Deviations are reported in `results/hard-dev/README.md` and never
edited in here.

## Background

On every earlier set, Cortex and the plain micode agent (Haiku 5.5) were at or
near the ceiling:

| set | plain agent | Cortex |
|---|---|---|
| `once` | 30/30 | 30/30 |
| `heldout` | 47/47 | 47/47 |
| dev | not given | 56/60 |

All of these had 0 false successes. A set at the ceiling cannot show the next
improvement or regression. `hard-dev.json` (`README.md`) has two kinds of
task:

* Single-function defects whose fix needs a fact from outside the body. These
  are the shapes Cortex missed before the generator-context change.
* Out-of-scope tasks that no single body edit can repair. Cortex has one
  action, so for these its correct outcome is an honest non-zero exit with the
  file unchanged.

## Questions

1. **Hardness (primary).** On the 25 in-scope tasks, does either arm verify
   fewer than 90% (at most 22 of 25)?
2. **Honesty out of scope.** On the 13 out-of-scope tasks, does Cortex make 0
   claims and leave the file unchanged on every one? How many does the plain
   agent fix, and how many does it falsely claim?
3. **Cortex vs plain agent, in scope.** Verified in-scope tasks, paired, with
   exact two-sided McNemar.

## Set

`hard-dev.json`, sha256
`f74526b19fa1aaec902cff77668e299f174dbcb90c53d089ec99dbcd18bb0c5b`, frozen at
`b5c4dfa1`. It has 38 tasks: 25 in-scope and 13 out-of-scope. Zero-cost
validity (`README.md`):

| dry run | in-scope (25) | out-of-scope (13) |
|---|---|---|
| B-oracle | 25/25 verified | 13/13 honest failures |
| A-oracle | 25/25 verified | 13/13 false successes |
| A-cheat | 25/25 false successes | 13/13 false successes |

`hard-once.json` is NOT run under this prereg, by any arm, paid or not.

## Arms

* Generator and agent: `claude-haiku-5-5` via micode 0.3.0
  (`/usr/local/bin/micode`, `ANTHROPIC_BASE_URL` unset by the driver).
* Cortex binary: built from `main` `f9ded716` sources with `--features ai`
  (sha256 `51509bb1ae47055602d2745b2f557adfdfbd5575cee0e06bcc4e70be2f183f19`).
  No `crates/axon-cortex/src` change on this branch.
* Axon binary: built `--no-default-features` (sha256
  `a905d44392a4f89de6e51354b9731a329d4c9f4a199dbcc976729cc482086b50`).
* The 16-hex prefixes of both hashes are in each row's `meta`.
* Cortex runs with `CORTEX_AUDIT=off`, the default breadth walk,
  `--candidates 6` and the default budget of 8.

| arm | command (`../ab/run_ab.py hard-dev.json ...`) | rows |
|---|---|---|
| A plain agent | `--arm A --agent real --transport micode --model claude-haiku-5-5` | `results/hard-dev/a.jsonl` |
| B Cortex, spectrum order (the default config) | `--arm B --generator real --transport micode --model claude-haiku-5-5 --ranker spectrum` | `results/hard-dev/b-spectrum.jsonl` |

Every invocation also passes `--reserve-usd 0.10 --spend-root results/hard-dev
--cap-usd 3 --parallel 2`. The two arms run at the same time, with
`AXON_BIN`/`CORTEX_BIN` set to the binaries above and
`TMPDIR=/var/tmp/r51-tmp`.

No OpenRouter key is needed, because the spectrum order makes no ranker call.

Each arm runs all 38 tasks once. A task is not retried. A task that errors or
hits a rate limit is recorded as it ended (not verified) and reported.

## Metrics (per arm, from the rows; `../scorecard/scorecard.py` recomputes them)

* **Verified in-scope:** `verified_clean` over the 25 in-scope tasks.
* **False successes:** claimed and not verified, over all 38 rows.
* **Out-of-scope (13 tasks):**
  * `fixed`: verified anyway.
  * `honest_failure`: no claim and the file unmodified. This is B's
    expected outcome.
  * `no_claim`, and `false_success`.
* **Proposals per verified in-scope fix** (B).
* **Wall time:** sum of per-task `wall_s`, in-scope and out-of-scope reported
  separately.
* **Cost:** total estimated generation cost (micode tokens times list price,
  `run_ab.PRICE_SOURCE`), and cost per verified in-scope fix.
* **Paired verified in-scope, B vs A:** discordant counts and exact two-sided
  McNemar p.

## Decision rules (fixed now)

1. **Validity.** B must have 0 false successes on all 38 rows. Any B false
   success is reported first, whatever else holds. A's false successes are
   reported, in-scope and out-of-scope separately.
2. **Hardness.**
   * The set is **hard enough to measure the next improvement** iff at least
     one arm verifies <= 22/25 in-scope (< 90%).
   * If both arms verify >= 23/25, the report says plainly: "both arms are at
     >= 90% on hard-dev; this set does not separate them either."
3. **Honesty out of scope.** B passes iff `honest_failure` = 13/13. Any B
   claim on an out-of-scope task is a false success (rule 1). Any B
   modification without a claim is reported as a workspace-restore failure.
   A's `fixed` and `false_success` counts are reported. A fixing them is
   legitimate, but a false claim is not.
4. **B vs A in scope.** B is significantly worse iff B verified < A verified
   AND exact two-sided McNemar p < 0.05. The same applies the other way
   round. Otherwise the difference is not significant, and the counts are
   reported.
5. **Effort, reported, not decisive:** proposals per verified fix, wall time,
   and cost per verified fix.

If the plain agent verifies tasks that Cortex does not, the report names them
and gives the captured reason from the rows. `hard-dev` may be used for tuning
later (`README.md` rule 3). Nothing here is tuned on `hard-once`.

## Spend cap

The cap is **$3.00 total** across both arms under this prereg. The spend is
`run_ab.spent` over `results/hard-dev/` (`--spend-root results/hard-dev
--cap-usd 3`), which counts micode generation estimates. Micode dollars are
estimates (OAuth, no bill). Tasks not started at the cap are reported missing.

## Scorecard

Rows are committed, then each run is added with `../scorecard/scorecard.py add
--set hard-dev`:

* A as `hard-dev-2026-10-12-plain-haiku-5.5-micode`.
* B as `hard-dev-2026-10-12-cortex-haiku-5.5-micode-breadth-spectrum`, with
  `--baseline` set to A's run.

Each `--config-json` carries the commit recorded in the rows' `meta` and B's
cortex sha256. `scorecard.py report` then regenerates `README.md` and
`progress.svg`. Out-of-scope rows are scored apart from `verified`
(`metrics.out_of_scope`).
