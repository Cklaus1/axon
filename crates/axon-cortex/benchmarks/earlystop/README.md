# Early stop: giving up sooner on what one function body cannot repair

Question: on defects Cortex cannot fix (the fix spans two functions, a
signature or a type; or the task is unsatisfiable), can it say so sooner,
without losing a fix on the ones it can? Rule and flags: `docs/CORTEX.md`,
"Giving up early". Code: `crates/axon-cortex/src/earlystop.rs`; conformance
tests: `crates/axon-cortex/tests/early_stop.rs`. Pre-registration:
`PREREG-earlystop.md` (committed alone in `ce7d7f4f`, before any paid row).

Model: `claude-haiku-5-5` via micode 0.3.0. Every figure below is computed from
the committed rows by the scripts named next to it.

| file | what |
|---|---|
| `results/measure/` | rule-ABSENT rows on this branch, made to record the signals (`hard-dev-b.jsonl`, `tempt-b.jsonl`); est. $0.097 + $0.061; before the prereg |
| `breakdown.py` -> `results/breakdown.txt` | where the seconds go, and the in-sample simulation the rule was chosen from |
| `reach.py` -> `results/reach.txt` | zero-cost check of the structural signal against every set |
| `results/dryrun/` | zero-cost dry runs of the final binary (committed before the prereg) |
| `results/paid/` | the paid rows (`hard-dev-b.jsonl`, `tempt-b.jsonl`), logs, `analysis.txt` (the verdict), `summary.txt` |
| `analyse.py` | the prereg's decision rule; `--selftest` |

## 1. Where the time went (before choosing a rule)

From the committed rows (`hardset/results/hard-dev/b-spectrum.jsonl`,
`tempt/results/paid/b-spectrum.jsonl`), `python3 breakdown.py`:

| scope | n | wall | model calls | other | proposals | candidates asked | first proposal of a candidate | every later proposal |
|---|---|---|---|---|---|---|---|---|
| hard-dev out-of-scope | 13 | 547.2 s | 515.8 s | 31.4 s | 110 | 30 | 30 (100.1 s) | **80 (415.8 s)** |
| hard-dev in-scope | 25 | 91.0 s | 73.7 s | 17.3 s | 29 | 25 | 25 (53.2 s) | 4 (20.5 s) |
| tempt unsatisfiable | 7 | 254.5 s | 245.0 s | 9.5 s | 41 | 14 | 14 (75.7 s) | **27 (169.2 s)** |
| tempt satisfiable | 23 | 161.9 s | 151.3 s | 10.6 s | 35 | 34 | 34 (140.9 s) | 1 (10.5 s) |

* 94% of the out-of-scope wall time is model calls (515.8 of 547.2 s); Cortex's
  own work is not the cost.
* 76% of the out-of-scope seconds (415.8 of 547.2) are proposals AFTER a
  candidate's first, which the satisfiable tasks barely make (4 of 29 and 1 of
  35 proposals). The breadth walk (1 proposal per candidate first) already
  makes the first pass cheap; the spend is pass 2 resuming wrong candidates.
* A satisfiable task spends 1.2 and 1.0 proposals per candidate asked (29 / 25
  and 35 / 34 in the table), an unsatisfiable one 3.7 (110 / 30) and 2.9
  (41 / 14).

Signals visible early, read from the rule-absent `results/measure/` rows (all
13 + 7 out-of-scope tasks, `reach.py` and `breakdown.py --simulate`):

| signal | out-of-scope/unsat tasks it is visible on | in-scope/satisfiable tasks it would touch |
|---|---|---|
| no function reached by every failing visible check | 7 of 13 hard-dev (all `two-functions-01..05, 08, 09`), 0 of 7 tempt | 0 of 48 |
| a candidate's visible failing set unchanged across its first N compiled proposals (any candidate) | see the simulation row below | 0 of 48 for N >= 2 |
| a compile error outside the target | not measured; the rows do not record where a diagnostic was | not measured |
| flat localization spectrum | not measured; the structural test replaces it (it is exact on the common-function question where a score gap is a guess) | not measured |

`reach.py` (zero cost: `cortex locate --json`, no model, no paid call) on every
set holding in-scope defects: the planted function is reached by EVERY failing
visible check on 60 of 60 dev-corpus tasks, 25 of 25 hard-dev in-scope, 47 of
47 `heldout`, 30 of 30 `once`, 19 of 19 `hard-once`, 30 of 30 `tempt`; on 0 of
211 is it missing (`results/reach.txt`). `hard-once.json` was read this way
only (no model call, no row); it stays unused for paid runs.

In-sample simulation (the rows the rule was designed from; replays one row,
a candidate stops after its Nth consecutive inert proposal, and unreached
candidates are not asked; the saved time is the recorded per-proposal API time
of proposals that would not be made; `results/breakdown.txt`):

| rows | inert N | structural | would save | in-scope would skip |
|---|---|---|---|---|
| hard-dev (rule absent) | 2 | off | 120.5 s of 615.1 s (19.6%), 17 proposals | 0 |
| hard-dev | 3 | off | 56.5 s (9.2%) | 0 |
| hard-dev | none | on | 471.4 s (76.6%), 83 proposals | 0 |
| hard-dev | 2 | on | 502.1 s (81.6%), 90 proposals | 0 |
| tempt | 2 | off | 87.2 s of 260.9 s (33.4%), 15 proposals | 0 |
| tempt | 3 | off | 34.5 s (13.2%) | 0 |
| tempt | 2 | on | 87.2 s (33.4%), 15 proposals | 5 proposals (29.6 s), in tasks that verified anyway |

Chosen: both signals, inert N = 2. N = 1 would stop a candidate on its first
refused proposal (no evidence yet about whether the verdict moves); N = 3
saves too little to matter on tempt (13.2%).

## 2. Result (PREREG-earlystop.md, in-sample)

`python3 analyse.py results/paid/hard-dev-b.jsonl results/paid/tempt-b.jsonl
--control results/measure/hard-dev-b.jsonl results/measure/tempt-b.jsonl` ->
`results/paid/analysis.txt`. 68 of 68 rows, nothing missing. Estimated spend
$0.0706 (cap $2.00) for this prereg's runs.

| criterion | measured | verdict |
|---|---|---|
| C0 0 false successes (68 rows) | 0; no claim on any of the 20 out-of-scope tasks | PASS |
| C1 (a) no in-scope task the committed rows verified ended `out_of_scope_suspected` | none | PASS |
| C1 (b) in-scope verified not significantly lower | 47/48 vs committed 47/48; lost 0, gained 0; McNemar p = 1.000 | PASS |
| C2 out-of-scope wall lower, one-sided exact Wilcoxon | 247.0 s vs 801.7 s over 20 tasks (-69.2%); p < 0.0001 (printed 0.0000) | PASS |

**Verdict: PASS.**

Per group (`results/paid/summary.txt`):

| group | n | committed wall | same-day rule-absent control | new wall | committed proposals | new proposals | new outcomes |
|---|---|---|---|---|---|---|---|
| hard-dev out-of-scope | 13 | 547.2 s | 615.1 s | **77.8 s** | 110 | 16 | 11 `out_of_scope_suspected`, 2 `budget_exhausted` |
| tempt unsatisfiable | 7 | 254.5 s | 260.9 s | **169.2 s** | 41 | 28 | 1 `out_of_scope_suspected`, 4 `budget_exhausted`, 2 `adjudicated_not_clean` |
| hard-dev in-scope | 25 | 91.0 s | 71.1 s | 76.8 s | 29 | 28 | 24 `verified_done`, 1 `out_of_scope_suspected` |
| tempt satisfiable | 23 | 161.9 s | 169.5 s | 143.8 s | 35 | 30 | 23 `verified_done` |

Where the saving comes from:

* **Structural stop (signal 1): 7 tasks, 0 model calls, 1.1 s in total**,
  against 373.8 s and 77 proposals in the committed rows for the same 7. Of
  the 13 hard-dev out-of-scope tasks it takes the whole `two-functions`
  group that has no common function.
* **Inert stop (signal 2): 9 tasks had a candidate stopped (12 candidates)**;
  it ended 6 runs as `out_of_scope_suspected`. On the other 13
  out-of-scope/unsat tasks (the structural stop did not apply), wall time fell
  from 427.9 s to 245.9 s.
* The 21 tasks with candidates pruned by signal 1 (not reached by every
  failing check) included 11 in-scope tasks; all 11 verified.

Still `budget_exhausted` (6 tasks, 3-9 proposals each): the signals did not
fire, and for stated reasons in the rows. `two-functions-06`: the visible set
changed between proposals (a proposal fixed one visible test and broke the
other), which is evidence of movement. `two-functions-07` and `unsat_doc-01`:
the failing visible set went to empty, that is, every visible check passed and
the hidden check still refused, which is also what an in-scope candidate the
hidden check dislikes looks like, so the rule cannot call it. `unsat_tests-03`,
`-04`, `unsat_doc-03`: the inert stop did fire (`stopped_candidates`:
`marginal_rate`; `cmp_i64`, `num`, `piece`, `piece_count`; `u_i64`), but other
candidates were still paused with budget left, so the run as a whole ended on
budget. On tempt's unsatisfiable tasks every candidate is reached by the failing
check, so the structural stop never applies, and the saving is smaller:
254.5 s to 169.2 s.

### The in-scope task the rule stopped

`hd-caller-contract-05` (`add_manager`, the CVE exemplar both arms had refused:
the plain agent made no claim, committed Cortex made 4 proposals and ended
`budget_exhausted`) ended `out_of_scope_suspected` after 2 proposals, both refused with
the visible set unchanged. It is not a C1 failure: the committed rows did not
verify it. But the same-day rule-absent control verified it on its FIRST
proposal (`verified_done`, 1 proposal, 4.7 s), so on that one task the outcome
depends on the model's sample, and the rule ends the unlucky sample sooner.
Against the control, in-scope verified is 47/48 vs 48/48 (lost
`hd-caller-contract-05`, McNemar p = 1.000), and `analyse.py` reports it as a
verified fix the rule stopped. What the rule did was end it before a third proposal. Whether a third would
have verified is not observable. This is the measured cost of the rule on this set: one task
in 48, on which the model's own behaviour is the variable.

### Caveats

* **In-sample.** Both sets are the ones the rule was designed from; the
  structural check on the other four sets (`reach.txt`) is a zero-cost,
  out-of-sample test of signal 1 only. Signal 2 has no out-of-sample
  evidence yet.
* **One run per task.** Wall time varies between runs of identical code: the
  rule-absent control is +9.3% over the committed rows on the 20
  out-of-scope tasks (876.0 s vs 801.7 s), and individual tasks move by
  2-3x (`two-functions-06`: 25.9 s committed, 69.3 s control, 16.3 s now).
  Other agents on this machine were running paid benchmark jobs at the same
  time as these runs. The -69.2% is far outside that noise and the same
  test against the control is also p < 0.0001 (-71.8%), but individual
  per-task times are not precise.
* **Different axon build.** The committed baseline used an axon from `main`
  `f9ded716`; this run's is from this branch (sha256 in the prereg). The
  interpreter is not what the rule changes.
* **Deviation, disclosed.** The two paid invocations were meant to run at the
  same time. When tempt finished, the hard-dev invocation had left an empty
  log and an empty rows file (0 bytes each), so it was started again alone.
  The rows in `results/paid/` are one complete run each, with no task run
  twice. The first invocation's spend, if any, is not in the spend figure
  (its rows file is empty, so nothing was counted).
* The measurement runs (`results/measure/`) were paid, rule-absent, and made
  before the prereg, to record the signals; est. $0.158, outside the
  prereg's cap.
