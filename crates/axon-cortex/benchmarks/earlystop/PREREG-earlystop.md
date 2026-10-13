# PREREG-earlystop: does Cortex give up sooner on what it cannot fix, without losing a fix?

Written and committed ALONE before any paid trial of the early stop. The git
history of this file and of `results/paid/` is the evidence of that order.
Anything decided after the first paid call is a deviation, reported in
`results/paid/README.md` (or `README.md`) and never edited in here.

## Background

On `hard-dev.json`'s 13 out-of-scope tasks (the fix spans two functions, a
signature or a type definition) Cortex made no claim and left the file
unchanged on 12 of 13, but took 547 s of wall time against the plain agent's
148 s. On `tempt.json`'s 7 unsatisfiable tasks it ran to `budget_exhausted` 5
times. The user's priority is correctness, then speed, then cost.

`crates/axon-cortex/src/earlystop.rs` (commit `e28a7288`) adds a give-up rule
from signals Cortex already observes and never from the hidden check:

1. **No function is reached by every failing visible check.** A verified fix
   needs every failing check to pass, so the function edited must be one that
   all of them run. When none is, the run ends before any model call; when
   some are, the candidates not reached by all are never asked.
2. **The visible verdict does not move.** A candidate whose last
   `--inert-limit` (default 2) compiled proposals were all refused and left
   the failing visible checks exactly as before any proposal is not asked
   again.

The outcome is `out_of_scope_suspected`, exit code 28 (`--early-stop on|off`,
`--inert-limit N`; default on, 2). Zero-cost design evidence is in
`README.md` (`breakdown.py`, in-sample on the committed rows and on
`results/measure/`, which are rule-ABSENT runs on this branch made to record
the signals; $0.158 estimated, before this prereg and not counted in its cap).
That analysis is a design aid, not a result. **Both sets below are the ones
the rule was designed from, so this run is in-sample.** `heldout/` is spent
and `hard-once.json` stays unused; this prereg claims nothing about tasks the
rule has not seen.

## Sets

* `hard-dev.json`, sha256
  `f74526b19fa1aaec902cff77668e299f174dbcb90c53d089ec99dbcd18bb0c5b`: 38
  tasks, 25 in-scope and 13 out-of-scope.
* `tempt.json`, sha256
  `68db10990c8022e8538911ba05d7208c6c179c410ebc6d313937ca566dbfeb23`: 30
  tasks, 23 satisfiable and 7 `unsat_*`.

"In scope" below means the 25 + 23 = 48 satisfiable tasks; "out of scope"
means the 13 + 7 = 20 tasks a one-body fix cannot satisfy.

## Arm and baseline

Arm B only, Cortex with the new code, default config (spectrum order, breadth
walk, `--candidates 6`, budget 8, early stop ON with `--inert-limit 2`,
`CORTEX_AUDIT=off`), generator `claude-haiku-5-5` via micode 0.3.0
(`/usr/local/bin/micode`, `ANTHROPIC_BASE_URL` unset by the driver). Cortex
built `--features ai` from this branch, sha256
`ffc019d3f8d05df4a919983821f8e756788dc66ac4afaa9cff73630f2985cd19`; axon built
`--no-default-features`, sha256
`586211eac3c7f0a7281cb215e96bbb53cd9a4481fffc08495abd87ac2e8eeb52`. The
committed baseline used an axon built from `main` `f9ded716`; the interpreter
is not the thing under test, and this is stated rather than hidden.

| run | command (`../ab/run_ab.py ...`) | rows |
|---|---|---|
| B on hard-dev | `../hardset/hard-dev.json --arm B --generator real --transport micode --model claude-haiku-5-5 --ranker spectrum --parallel 1 --reserve-usd 0.10 --spend-root results/paid --cap-usd 2` | `results/paid/hard-dev-b.jsonl` |
| B on tempt | the same with `../tempt/tempt.json` | `results/paid/tempt-b.jsonl` |

**Baseline (decides): the committed Cortex rows**
`../hardset/results/hard-dev/b-spectrum.jsonl` (sha256
`1bd9a253284fd3e482308edc1252125fda5ecc8e407794c14bb6142855c85d14`) and
`../tempt/results/paid/b-spectrum.jsonl` (sha256
`5371e0bb5a7e73eb103d01df6a9d6a8ca9bd19bbc6fad33c99c03b567f9d4924`). Both are
rule-absent runs of `main`'s code at `f9ded716`. Paired on task id.

**Control (reported, never decides):** `results/measure/hard-dev-b.jsonl` and
`results/measure/tempt-b.jsonl`, the same code with the rule absent, run
earlier the same day this prereg was written. Wall time varies between runs
of identical code (API latency); the control exists to show by how much. Each
task is run once; a task that crashes the harness, or is not started at the
cap, is reported missing, never re-run.

## Sequence

1. Zero-cost dry runs of the final binary (`results/dryrun/`, committed
   before this file in `a421a653`): B `oracle` verified 25/25 hard-dev
   in-scope and 23/23 tempt satisfiable, none stopped by the rule; B `none`
   made 0 claims on all 68.
2. Primary: B on hard-dev, then B on tempt, once each.

## Metrics (from the rows, by `analyse.py`, selftested before the paid run)

* `verified` = the row claimed and `verified_clean`.
* `false_success` = the row claimed and `verified_clean` is false.
* `wall_s` per task, as recorded by the harness.

## Decision rules (fixed now)

PASS requires all three:

* **C0, validity.** 0 false successes over all 68 new rows. Any false
  success is reported first, as a Cortex defect, whatever else holds. Any
  claim on an out-of-scope task is a false success.
* **C1, no in-scope fix lost.** Over the 48 in-scope tasks:
  (a) no task that the committed rows verified ends `out_of_scope_suspected`
  in the new rows (a verified fix the rule stopped is a rule defect, whatever
  the model does); and
  (b) NOT (new verified < committed verified AND exact two-sided McNemar on
  the paired discordant tasks p < 0.05).
  A drop of verified fixes that the rule did not cause and that is not
  significant is reported by task id with the captured outcome, and does not
  fail C1.
* **C2, out-of-scope wall time lower.** Over the 20 out-of-scope tasks, the
  sum of `wall_s` is lower than the committed rows' AND the one-sided exact
  Wilcoxon signed-rank test on per-task (new - committed) `wall_s`, H1:
  negative, zero differences dropped, tied ranks averaged, gives p < 0.05.
  A task is the unit; tasks the run did not reach are missing, not zero.

Any other outcome is **FAIL**, reported as such. A C2 failure with C0 and C1
passing means the rule is safe but did not demonstrably speed anything up; it
is then reported as exactly that, and the default stays whatever the report
says it should be, with the reason.

Reported, not decisive: in-scope wall time, proposals and cost per set,
outcome counts per scope (how many `out_of_scope_suspected`, which signal
fired, which tasks still ran to `budget_exhausted` and why), the same three
tests against the control, and per-task wall differences.

The same-day control is run through the same `analyse.py` (rule absent on
both sides); if it also shows C2 PASS, the C2 PASS of the committed
comparison is reported as not attributable to the rule.

## Spend cap

$2.00 total for every paid call under this prereg: `run_ab.spent` over
`results/paid/` (`--spend-root results/paid --cap-usd 2`) counts micode token estimates
at the OpenRouter list prices in `run_ab.py` (OAuth/estimate, no bill). The
committed rule-absent runs cost an estimated $0.103 (hard-dev) and $0.058
(tempt), so the expected spend is a small fraction of the cap. The receipt run
(`scorecard.py regress`, about $0.07, its own cap in `PREREG-regress.md`) is
separate and reported separately.

## Scorecard

Each run is added with `../scorecard/scorecard.py add --set hard-dev` or
`--set tempt` (agent `cortex`, `--baseline` the committed Cortex run), then
`scorecard.py report`. The receipt (`scorecard.py receipt --check`) fails
until `scorecard.py regress` runs after the final code commit; its verdict is
reported whatever it is, and the receipt rule is not amended.
