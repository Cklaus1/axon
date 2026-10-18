# hx pre-registration

Status: **DESIGN — not frozen.** Every `TBD` below is filled in, in one commit,
before the first model run it governs (the seeds and binaries before the pilot,
the cells and power output before the freeze, the FU-10 patch sha before any T2
run). After the freeze this file only gets appended result rows.

Source of the design: `tasks/followup-spec.md` FU-1c, FU-1f, FU-1h and FU-10
(including every [REVISED] marker). Code: this directory (README.md).

## 1. Pool, split, freeze (FU-1c)

| field | value |
|---|---|
| builder binaries (built from a tree containing FU-7) | axon sha256 `819e04609b49c2b329edb2f79bd5a7653d5ee59ed8ddf5809a29c3247924ddda`, cortex sha256 `e052ea815762ad13a6417e94efa5d3f5c08854c483c320da1fddeab6087a433a` (fu/int `b789a8f6` sources; copies at `/var/tmp/r51-tmp/hx-bin/`) |
| builder-input digest (`build.py --inputs-digest`) | `fbe27d3b45300843c8cd8deaf30ad46f3b4dec75e719fa8aca2dc5c7708f8eff` |
| mutation seed (`build.py --seed`) | 1 |
| split seed (`build.py --split-seed`) | 1 |
| extension split seeds (new programs only), if any | none. The 24 programs added after the first scratch build (`f9834709`, `d853e064`) were written before any model run on any hx task; the pool was built once over all 46 stratum-C and the stratum-D programs, so there is one split. |
| cross-fit fold seed (`calibrate.py --seed`) | 1 |
| per-program cap draw seed (`calibrate.py --cap-seed`) | 1 |
| pool: programs / tasks per stratum | C: 45 programs / 564 tasks (`validate.ax` rejected: ungradeable); D: 11 programs / 49 tasks (7 dropped, < 3 valid tasks); total 613 tasks, pool sha256 `c5e9f812e1144bb7f30d1d9f5edeca2268238724cf25d3123cef45fa2995a426` (`--stratum all --jobs 16`, `--per-cell-program 2`) |
| split: dev programs / test programs | 18 (13 C, 5 D) / 38 (32 C, 6 D); `split_ok` true (required: test >= 20, dev >= 10) |

1. The pool is built by `build.py`, split by source program: all tasks of one
   program go to one split. The split is a seeded shuffle; dev gets
   max(10, G/3) programs, test the rest. Programs with fewer than 3 valid tasks
   are dropped before the split.
2. Each test program contributes at most 8 tasks. The extra tasks are dropped
   by a seeded draw, after the kept-cell filter and before any test run, never
   by outcome.
3. If the kept test pool is too small for section 3, more pool is built with
   the same recipe: new programs get a new seeded split (seed recorded above);
   new mutation seeds on existing programs inherit that program's split. No
   program run on dev ever enters test.
4. Freeze: `hx-test.json` is committed, its sha256 pinned in
   `PROTECTED_SETS` (`../protected.py`); k per model (section 3) is fixed here
   first.

## 2. Pilot and calibration on dev (FU-1c)

| field | value |
|---|---|
| pilot (20 hx-dev tasks, h55 k=1, h45 k=1): cost per task-trial by outcome | `TBD` |
| pilot wall time, rate-limit rows | `TBD` |
| incumbent | f26e9506 cortex, arm B |
| primary model, k on dev | `claude-haiku-5-5`, k = 3 |
| cells: final (class, knob) list | `TBD` |
| eligible cells (>= 12 dev tasks from >= 3 dev programs) | `TBD` |
| prior (beta-binomial fit across cells): mean, precision | `TBD` |
| kept cells (shrunk rate in [0.3, 0.7]) with shrunk and raw rates | `TBD` |
| cross-fitted predicted test rate (required in [0.2, 0.8]) | `TBD` |
| h45 kept-cell dev rate (k = 1); informative iff in [0.1, 0.9] | `TBD` |

- Cells are (class, knob): multi 2|3 mutations; caller depth 1|2; subtle
  operator id; refine refine|verify; caps `-`.
- Cell rates are empirical-Bayes shrunk (beta-binomial across cells, evidence
  per task). A cell is kept iff its shrunk rate is in [0.3, 0.7].
- Cross-fit: dev programs are split 2-fold (seeded); cells are selected on one
  fold and measured (raw) on the other, both ways. The out-of-fold kept-cell
  rate, weighted by the cell counts of the capped test set (the tasks that
  will be run), is the predicted test rate.
- Test tasks are selected by cell membership only. hx-dev (dev tasks with
  mixed outcomes) is for development, never a calibration source.

## 3. Power and design (FU-1f)

| field | value |
|---|---|
| `power.py` command line and seed | `TBD` |
| `power.py` output (verbatim, committed) | `TBD` |
| chosen design: n_test, G_test, k (h55) | `TBD` |
| k on h45 | `TBD` (planned 1) |
| power at sigma_g = 0 / 0.5 / 1.0 | `TBD` |
| power under the flip model | `TBD` |
| C-only secondary row: n, programs, power | `TBD` |
| P(delta_2 > 0) for h45 at the chosen design | `TBD` |
| estimated cost vs. the $25 cap | `TBD` |

- Statistic: task-weighted mean of the per-task difference in k-trial success
  rate (candidate − baseline, same tasks).
- p-value: one-sided sign-flip test on program sums; exact enumeration when
  2^G <= 65 536, else 10 000 seeded Monte Carlo flips. 90% CI by inversion of
  the same test.
- Simulation: per-task baseline rates are beta-binomial posterior means of the
  dev counts; effect = logit shift beta + u_g + e_i with u_g ~ N(0, sigma_g^2);
  beta gives a mean gain of +10 points. Sized for power >= 0.80 at
  sigma_g = 1.0, within the cost cap. Inside the simulation the sign-flip
  null is approximated by its normal limit (`power.py --exact` cross-checks).
- [EST] planning numbers, replaced by the output above: G_test >= 20, <= 8
  tasks per program, n_test ≈ 160, k = 5 on h55, k = 1 on h45.

## 4. Gain criterion (FU-1h)

A candidate shows **meaningful task-solving improvement** on hx-test when all
hold:

- (i) primary model `claude-haiku-5-5`: one-sided sign-flip p < 0.05 and
  Δ̂ >= +5 points;
- (ii) false successes summed over all n × k primary trials <= T0's;
- (iii) second model `claude-haiku-4-5-20251001`: Δ̂₂ > 0, provided h45's
  kept-cell dev rate (section 2) is in [0.1, 0.9]; otherwise (iii) is recorded
  as "uninformative" and the criterion rests on (i) and (ii).

The conditions form an intersection, so the familywise α stays <= 0.05.
Spending hard-once still needs explicit human approval (NH-R2 = A).

**Contamination (FU-1b), descriptive only.** Per-stratum incumbent rate and
per-stratum Δ with a test-inversion CI; one-sided D − C program-label
permutation test (10 000 permutations), `contamination-suspect` iff p < 0.05,
reported next to the result. The criterion stays on the pooled set; the C-only
Δ is a secondary row with the power stated in section 3.

**Known confound (A#17).** The incumbent's early stop counts a proposal as
inert when it leaves the visible failing set unchanged. A partial fix of a
`multi` task can look inert, so `multi` headroom partly measures the
early-stop heuristic. The `multi` cell rates are reported separately: `TBD`.

## 5. FU-10: typed-feedback (R4) keep/remove

| field | value |
|---|---|
| T2 branch | `fu/r4-ablation` |
| T2 patch sha (revert of 3ffc816b, 015f16b7, fd032a6d's feedback parts) | `TBD` |
| T2 sealing tests and `cargo test -p axon-cortex` evidence | `TBD` |

Arms on hx-test, in the FU-11 run: T1 = the merged head (typed R4), T2 = the
merged head with R4 reverted.

- **KEEP** if either route holds:
  - Route 1: Δ(T1 − T2) one-sided sign-flip p < **0.025** on h55, and h45's
    Δ̂₂ >= −5 points. The guard's false-veto rate at Δ₂ = 0 is ≈ 0.19 at
    k₂ = 1.
  - Route 2 (efficiency): per task, the mean proposals over verified trials
    in each arm, over tasks with >= 1 verified trial in both arms;
    d_i = T2 − T1; one-sided sign-flip by program p < **0.025**, and Δ̂ >= 0
    on solve rate.
- **REMOVE** otherwise. Power: MDE₈₀ ≈ 7 points; REMOVE means "no gain of
  >= ~7 points shown", not "no effect", and the report says so.

Computed by `stats.py compare` / `stats.py route2` and `stats.fu10_verdict`.

## 6. Results (append only, after the freeze)

None yet.
