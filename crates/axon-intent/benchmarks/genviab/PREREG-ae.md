# PREREG: authoring-eval (`ae`) - pre-registration

**Status: FROZEN 2026-11-25 (after the pilot, the A/A run and HR-2; spec AE-3 "Pilot, then freeze")**

`ae.py run` refuses every non-`--pilot` run until the status line reads `**Status: FROZEN ...**` and the pins below are filled.
The pins are read verbatim (`- key: \`value\``); a run whose manifest sha or served model series differs is refused (spec AE-4).

## Pins (filled at freeze)

- set version: `ae-1.0`
- A/A null SD: `0.0179` (headline FT-pass Δ, as a fraction; the weekly screen's rerun trigger is |Δ| ≥ 2.5 × this)
- manifest sha256: `5015c9ff1432548808990e55cd13ac2a214a1f93d5f1dd1334ef6437dfb50008` (`<private>/ae-1/frozen/manifest.json`)
- model series: `cere:Qwen3.8-Flash-Next:796f2f99a2fe5c02:722357d5e54ae8d8` (genviab `http_series`)

## Design (canon: tasks/authoring-eval-spec.md §2, §3, D1-D12)

- Measured model: CERE Qwen (`Qwen3.8-Flash-Next`), `--provider cere`, temperature 0.7, `max_tokens` 8192, seeds 0-5 (k = 6;
  raised from 3 by AE-3's power rule, see "A/A run" below).
- Headline: M1 FT-pass (round-0 candidate passes `axon check`, the gates and every visible and hidden example), paired Δ
  candidate - base over tasks, family-clustered sign-flip (`headroom/stats.py` `SignFlip`), 95% CI = `ci(alpha=0.025)`,
  two-sided p. Secondary: Solve@3. Infra rows leave every denominator; > 5% infra writes no trend row.
- Flags: `REGRESSION-UNCONFIRMED` (CI entirely below 0) until a `--confirm` run on fresh seeds (cache off) also has its CI
  entirely below 0 (`REGRESSION`) or touches 0 (`NOT CONFIRMED`); `IMPROVED`; `NO CHANGE`. The first 3 labelled releases and
  every weekly screen row are `advisory`.
- Python baseline (arm c): every release and confirm run, on the run's seeds (k = 6; `ae.py` runs arm c once per seed).

## Pilot table (filled at freeze; the final draw, 6 trials per task: attempt 2 seeds 0-2 + A/A seeds 3-5)

| tier | n | Qwen FT-pass | Qwen Solve@3 | arm c FT-pass | ρ | A/A Δ / null SD (pts) | power at 4/8 points (k = 6) |
|---|---|---|---|---|---|---|---|
| T1 | 60 (40 fam) | 0.414 | 0.711 | 0.908 | 0.18 | +3.89 / 3.97 | 0.20 / 0.63 |
| T2 | 80 (57 fam) | 0.162 | 0.592 | 0.867 | 0.07 | +3.33 / 3.28 | 0.37 / 0.89 |
| T3 | 60 (43 fam) | 0.075 | 0.292 | 0.689 | 0.11 | +1.67 / 2.66 | 0.46 / 0.94 |
| TS | 40 (26 fam) | 0.188 | 0.596 | 0.925 | 0.13 | +2.50 / 4.64 | 0.17 / 0.58 |
| set | 240 (166 fam) | 0.208 | 0.547 | 0.842 | 0.21 | +2.92 / 1.79 | 0.81 / 1.00 |

- ρ_fam is not reported (see "A/A run"); power clusters by family directly. Per-tier rows are descriptive; only the set
  row is tested (headline). Per-tier power is below 0.8 and is not claimed.
- A/A on the set: Δ +2.92 pts, family-clustered `SignFlip` p (two-sided) ≈ 0.11, 95% CI [-0.57, +6.35]: covers 0. Seeds
  3-5 ran slightly above seeds 0-2 on every tier; a paired A/B uses the same seeds on both arms, so a seed-level shift
  cancels in Δ.

## Defect gates and difficulty tuning (spec AE-3)

- Recorded at freeze:
  - dropped for ambiguity after HR-2: none (43 reviewed, 5 intent fixes with outputs unchanged, 38 ok);
  - tiers re-piloted: none (human decision 2026-11-24, below);
  - tiers shipped outside their band at their measured rate: T1 (0.414 < 0.85), T2 (0.162 < 0.4), T3 (0.075 < 0.15).

## Pre-pilot record (appended 2026-11-11, before any pilot call)

- **Pool:** 347 candidates (T1 89, T2 113, T3 86, TS 59; 179 families), private `ae-1/candidates` at private commit `59191d3`.
  - pool manifest sha256: `84921beab583fd021b31349da979941892fc22a079a431a61ea85c03cc7f574f`
- **HR-1 human review** (spec AE-3, reduced review decided 2026-11-11):
  - reviewed: every flagged candidate (48), every `needs-human` candidate (7), and a seeded 10% sample of the other 299 (30);
  - sample bad count **4 / 30**, which is > 2, so all 299 were reviewed;
  - result: 7 dropped (the `needs-human` candidates, which had no reference), 37 intent wording fixes and 310 unchanged;
  - no fix changed an expected output; `build.py --check` passes on all 347.
- **Draw seed:** `9114123031242144058`, the first 16 hex digits of `sha256("ae-1-draw:" + pool manifest sha256)` read as an
  integer.
  - After the pilot, each tier is drawn from its survivors: order them by `sha256("<seed>:<id>")` and take the first n
    (T1 60, T2 80, T3 60, TS 40).
  - If a tier has fewer than n survivors, it ships with all of them.
- **Pilot binaries** (both bins, so `regrade`): `axon 0.1.0 (a01db50c)`.
  - sha256 prefix: `axon` `fa490a5cb59262c4`, `axon-intent` `47069ddb911d6ee4`.
- **Pilot runs:**
  - `ae.py run --pilot --seeds 0,1,2` (k = 3, arms a and c);
  - then the A/A run `--seeds 3,4,5` with the same binaries.
  - A task that fails all 6 trials goes to a second human review, for ambiguity only.

## Pilot attempt 1 (recorded 2026-11-11, before attempt 2): INVALID, not used

- Run `20261111-132804-pilot-k3`, series `cere:Qwen3.8-Flash-Next:796f2f99a2fe5c02:b73b17648ae394fe`, 4 h 59 min.
- **INVALID: infra 270 / 3123 > 5%.** 265 of the 270 infra rows are seed 2: production CERE was re-deployed during
  it (240 `HTTP 503 not_ready: backend not ready: gpu1: probe`, 22 connection refused, 1 `shutting_down`; 7 more are
  `finish_reason 'repetition'`).
- Seed 2's other calls came from a new server build (`722357d5e54ae8d8`, 1407 calls; 168 still from `b73b17648ae394fe`).
  The driver did not check this, so those rows mixed two series. Fixed before attempt 2: a reply from another build
  than the series pins is infra (`build_changed`) and never cached (`drivers/http-program.py`, selftest `ae1`).
- `ae.py report` overflowed in `binom_cdf` at n = 1041 (`math.comb` exceeds a float). Fixed: summed in log space,
  equal to exact rational arithmetic within 6e-14 (selftest checks the n = 1041 interval).
- No number from attempt 1 is used for tiers, ρ, the draw or the A/A SD. Attempt 2 repeats the pilot runs above
  unchanged (same pool, binaries, seeds 0,1,2) on the series current at its start.

## Pilot attempt 2 (recorded 2026-11-24): valid; tiers below band, decision open

- Run `20261111-183515-pilot-k3-a2`, series `cere:Qwen3.8-Flash-Next:796f2f99a2fe5c02:722357d5e54ae8d8` (still served
  2026-11-24), 5 h 20 min, $0.
  - Infra 4 / 3123 (all `finish_reason 'repetition'`).
  - 0 `build_changed` calls; 0 canary hits.
- Round-0 FT-pass (arm a, scored rows) against the AE-3 bands; Python is arm c:

| tier | band | n | FT-pass | check-fail | sandbox `./out` | Solve@3 | Python FT-pass |
|---|---|---|---|---|---|---|---|
| T1 | ≥ 0.85 | 267 | 0.378 | 0.281 | 0.037 | 0.667 | 0.903 |
| T2 | [0.4, 0.75] | 339 | 0.153 | 0.599 | 0.053 | 0.563 | 0.858 |
| T3 | [0.15, 0.5] | 256 | 0.055 | 0.715 | 0.043 | 0.289 | 0.690 |
| TS | any | 177 | 0.203 | 0.503 | 0.079 | 0.616 | 0.915 |

- T1-T3 are all below their bands. Python solves the same tasks at 0.69-0.90, and most Axon misses fail `axon check`
  (top codes E0001, E0102, E0000, E0303, E0403).
  - Every sandbox refusal is `dir_create("./out")` under the grant `./out/`: intended behaviour
    (`tasks/containment-b4-spec.md` CBA-2), scored as an Axon failure.
- **Tier decision:** `[DECIDED by human 2026-11-24: (a) - each tier ships at its measured rate; no re-pilot]`.
  - Why: AE-3's re-pilot knob (size, steps, error paths) tunes task difficulty. The tasks are not too hard: Python solves
    them at 0.69-0.90.
  - The misses are Axon-surface failures (`axon check` refusals), which is what the eval exists to measure. Easier
    variants would hide them.
  - This waives an AE-3 step. AE-3 (`tasks/authoring-eval-spec.md:409-413`) ships a tier at its measured rate only after
    one re-pilot with easier variants; the human skipped that re-pilot. The "T2 or T3 ≥ 0.85" no-headroom stop does not
    apply (T2 0.153, T3 0.055).
  - Rejected: (b) re-pilot as AE-3 is written; (c) re-band by Solve@3 or by the Python-arm rate.
  - Next: the A/A run (seeds 3,4,5, same pool, binaries and series), then the pilot table and the freeze.

## A/A run (recorded 2026-11-25): valid; power below 0.8 at k = 3

- Run `20261124-163634-pilot-aa-k3`, seeds 3,4,5, same pool, binaries (`a01db50c`) and series as attempt 2, 5 h 20 min, $0.
  - Infra 0 / 3123; every call served by `722357d5e54ae8d8`; 0 canary hits.
- Over 6 seeds (attempt 2 + A/A, arm a), per tier:

| tier | n tasks | Qwen FT-pass | Qwen Solve@3 | arm c FT-pass | ρ (task) | A/A Δ (pts) | A/A SD (pts) |
|---|---|---|---|---|---|---|---|
| T1 | 89 | 0.388 | 0.687 | 0.919 | 0.19 | +1.87 | 2.78 |
| T2 | 113 | 0.158 | 0.574 | 0.860 | 0.09 | +0.88 | 2.49 |
| T3 | 86 | 0.064 | 0.283 | 0.711 | 0.09 | +1.94 | 2.09 |
| TS | 59 | 0.229 | 0.607 | 0.918 | 0.13 | +5.08 | 4.41 |
| pool | 347 | 0.206 | 0.537 | 0.848 | 0.21 | +2.11 (p 0.15, 95% CI [-0.62, +4.87]) | 1.41 |

- ρ is the within-task correlation of the 6 FT-pass trials (beta-binomial moments). ρ_fam is not reported: with
  1-2 tasks per family (168 pairs, 11 singletons) the moment estimator is unstable (0.42-0.93 across tiers). The power simulation clusters by
  family directly instead.
- A/A Δ = seeds 3,4,5 minus seeds 0,1,2 per task, family-clustered `SignFlip` (179 families). It covers 0: no drift.
- **Power** (the spec's 4-point rule; simulated on the planned draw before HR-2 drops; per-task rates drawn from the
  6-trial posterior, additive Δ, family-clustered test, 2000 reps; `calib2.py` itself was never committed):

| k | 3 pts | 4 pts | 5 pts | 8 pts |
|---|---|---|---|---|
| 3 | 0.35 | 0.51 | 0.72 | 0.97 |
| 6 | 0.57 | 0.81 | 0.94 | 1.00 |
| 9 | 0.75 | 0.93 | 0.99 | 1.00 |

  - Size at Δ = 0: 0.04-0.05. Power at 4 points with k = 3 is 0.51 < 0.8, so per AE-3 k rises before n: **k = 6**
    (0.81). The pins and the A/A SD are fixed after HR-2, on the final draw.
- **HR-2:** 43 tasks fail all 6 trials (T1 7, T2 8, T3 23, TS 5; 29 in the planned draw; 3 also fail every Python
  trial, 5 never pass Python's first round). They go to the second human review, for ambiguity only.
  - `[DECIDED by human 2026-11-25: HR-2 verdicts as recommended - 5 fix, 38 ok, 0 dropped; message-stream fixes say
    stdout]`. The 5 fixes add one intent sentence each (placeholder words in reasons; every message goes to stdout; the
    call forms of one mini-language); no expected output changed. `build.py --check --candidates` 347 PASS after them.
    Private record: `ae-1/review/HR2-all-fail.md` + `hr2-cards/` at private commit `a10d6d1`.

## Freeze (2026-11-25)

- Final draw = the planned draw (HR-2 dropped nothing): 240 tasks (T1 60, T2 80, T3 60, TS 40; 166 families), copied to
  `<private>/ae-1/frozen/`; `build.py --set-dir <private>/ae-1/frozen` 240 PASS, 0 excluded, corpus `f95c321756fbf3a9`.
  Runs use `--set-dir <private>/ae-1/frozen` (the `ae-1/` root also holds the pool and review files).
- k = 6 (seeds 0-5) for every non-pilot run; `--confirm` takes the next 6 unused seeds.
- Series check at freeze: `http_series` returned the pinned series (2026-11-24 22:28 EST).
- After the freeze this file only gets appended rows.

## AG-4 planned run (appended 2026-11-25, before the run)

- One-off advisory frozen-set A/B (`tasks/authoring-gap-spec.md` AG-4), no trend row: `ae.py run --advisory --label
  ag-card2 --expect-card-change --seeds 0,1,2,3,4,5 --set-dir <private>/ae-1/frozen`.
- Base = `52279a84` (card 1, the last `main` before the AG lane): `axon` `817cf0187180fa23`, `axon-intent`
  `b981f5b26f6e5f1d`, card sha256 `b70a0b560b5b84eb` (the pilot's card).
- Candidate = `002bf554` (card 3 + AG-2 help rows; strict gate rc 0, 8329 passed / 0 failed): `axon`
  `80972410a4718b91`, `axon-intent` `29df4f8d69639cc8`, card sha256 `af9665caf24bdd19`.
- Headline and test as in "Design": paired M1 FT-pass Δ, family-clustered `SignFlip`, two-sided, k = 6.

## AG-4 result (recorded 2026-11-25): valid; IMPROVED (advisory)

- Run `20261125-080201-ag-card2`, 08:02-17:07, $0. Series `…:722357d5e54ae8d8` (checked at start); infra 5 / 4320;
  0 `build_changed`, 0 `model_substituted`, 0 canary hits. Written to `history.jsonl` as an advisory row (the plan above
  said "no trend row"; `ae.py --advisory` writes one, flagged advisory).
  - Note D4: one cache replay mismatched, so the cache was off for the rest of the run. That makes later calls fresh on
    both arms (the wall time doubled vs the estimate); it does not bias the pairing.
- **Headline:** M1 FT-pass 0.211 -> 0.408, Δ **+0.197** [95% CI +0.166, +0.229], p (two-sided) < 0.001, 240 tasks /
  166 families -> `IMPROVED`. Solve@3 Δ +0.148 (0.556 -> 0.704). Turns to green 1.02 -> 0.60.

| tier | n (task-trials) | FT-pass base | FT-pass cand |
|---|---|---|---|
| T1 | 360 | 0.417 | 0.669 |
| T2 | 480 | 0.165 | 0.371 |
| T3 | 358 | 0.081 | 0.182 |
| TS | 239 | 0.188 | 0.431 |

- FT-check (round 0 passes `axon check`): 0.46 -> 0.61. Round-0 example runs refused by the sandbox: 329 -> 1
  (the `dir_create("./out")` rule). Round-0 example runs with wrong output: 1150 -> 770.
- Check codes that fell most (round 0, base -> cand): E0001 635 -> 280, E0102 436 -> 243, E0000 322 -> 242, E0403
  226 -> 122. Rows aimed at by card 3: `arr_new` 47 -> 10, `str` name 25 -> 1, `:`-record literal 47 -> 11, `?` outside
  `Result` 61 -> 37. Unchanged: `_` in a pattern position (65 -> 64). Up: E0301 `str` ordered with `<` (19 -> 28).
- Soundness: M2s 1/665 -> 0/874. M2 CCP (check-clean programs that panic) 22/665 -> 21/874.
- Python arm c: FT-pass 0.849, Solve@3 0.910. The gap (Python - Axon card 3) is 0.440 [+0.399, +0.483] on first try and
  0.205 on Solve@3 (it was about 0.64 / 0.35 with card 1).
- Per-tier rows are descriptive (per-tier power < 0.8, see the pilot table).
- Correction (2026-11-25, after the result was recorded): the "Unchanged" clause of the check-codes bullet misreads the
  report: `Ident("_")` in its code table is the placeholder for any identifier, not a `_` habit. That row (`expected LBrace`, 65 -> 64) is
  `x is Err/Ok/Some/None` on 63 of 64 card-3 programs (40 of 65 on card 1, where most of the rest were `not f(..)`). Programs using
  `is` rose 44 -> 70 of 1440. Card 4 (`tasks/authoring-gap-spec.md` AG-5) targets it; it is not part of this run.

## AG-5 planned run (appended 2026-11-25, before the run)

- One-off advisory frozen-set A/B (`tasks/authoring-gap-spec.md` AG-5): `ae.py run --advisory --label ag-card4
  --expect-card-change --seeds 0,1,2,3,4,5 --set-dir <private>/ae-1/frozen`. `[AMENDED 2026-11-25 21:20, before the
  run: human decision "ignore blackout" - started at once with `--ignore-blackout` (recorded as `blackout_ignored` in
  the run's meta) instead of 08:02 2026-11-26. Risk stated: a CERE redeploy during the nightly trainloop makes its rows
  infra (`build_changed`); infra > 5% voids the run.]`
- Base = `002bf554` (card 3, the AG-4 candidate binaries): `axon` `80972410a4718b91`, `axon-intent` `29df4f8d69639cc8`,
  card sha256 `af9665caf24bdd19`.
- Candidate = `b2d22ab3` (card 4; strict gate rc 0, 8329 passed / 0 failed): `axon` `81a91979e1be420a`, `axon-intent`
  `31e42d67d98f17ef`, card sha256 `b17f46c270de5568`.
- Headline and test as in "Design": paired M1 FT-pass Δ, family-clustered `SignFlip`, two-sided, k = 6.
- Prediction, written before the run: card 4 aims at about 80 of 1440 round-0 programs (`is` 63 first errors, `str`
  ordering 17). If all of them then pass `axon check` and half of those pass every example, Δ ≈ +3 points, below the
  4-point change the design detects with power 0.81. Expected flag: `NO CHANGE`; the class counts (`is`, E0301 `str`)
  are the evidence that matters. A drop in M1 would be a regression in the card.
- The same run re-measures O-AG-03 (the trailing-newline share of wrong-output rows) on card 4.

## Not in this file

No task text, example, reference, family idea or canary (spec AE-3 contamination rules); those live only in the private store.
