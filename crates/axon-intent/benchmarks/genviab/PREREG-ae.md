# PREREG: authoring-eval (`ae`) - pre-registration

**Status: DRAFT (skeleton; the integrator freezes it after the pilot, spec AE-3 "Pilot, then freeze")**

`ae.py run` refuses every non-`--pilot` run until the status line reads `**Status: FROZEN ...**` and the pins below are filled.
The pins are read verbatim (`- key: \`value\``); a run whose manifest sha or served model series differs is refused (spec AE-4).

## Pins (filled at freeze)

- set version: `<ae-1.0>`
- A/A null SD: `<headline FT-pass Δ SD of the pilot's A/A run>` (the weekly screen's rerun trigger, |Δ| ≥ 2.5 × this)
- manifest sha256: `<sha256 of <private>/ae-1/manifest.json>`
- model series: `<cere:Qwen3.8-Flash-Next:<models-entry sha16>:<server-build sha16>>` (genviab `http_series`)

## Design (canon: tasks/authoring-eval-spec.md §2, §3, D1-D12)

- Measured model: CERE Qwen (`Qwen3.8-Flash-Next`), `--provider cere`, temperature 0.7, `max_tokens` 8192, seeds 0,1,2 (k = 3).
- Headline: M1 FT-pass (round-0 candidate passes `axon check`, the gates and every visible and hidden example), paired Δ
  candidate - base over tasks, family-clustered sign-flip (`headroom/stats.py` `SignFlip`), 95% CI = `ci(alpha=0.025)`,
  two-sided p. Secondary: Solve@3. Infra rows leave every denominator; > 5% infra writes no trend row.
- Flags: `REGRESSION-UNCONFIRMED` (CI entirely below 0) until a `--confirm` run on fresh seeds (cache off) also has its CI
  entirely below 0 (`REGRESSION`) or touches 0 (`NOT CONFIRMED`); `IMPROVED`; `NO CHANGE`. The first 3 labelled releases and
  every weekly screen row are `advisory`.
- Python baseline (arm c): every release and confirm run, k = 3, once per run.

## Pilot table (filled at freeze)

| tier | n | Qwen FT-pass | Qwen Solve@3 | arm c FT-pass | ρ | ρ_fam | A/A null SD | power at 4/8 points |
|---|---|---|---|---|---|---|---|---|
| T1 | | | | | | | | |
| T2 | | | | | | | | |
| T3 | | | | | | | | |
| TS | | | | | | | | |

## Defect gates and difficulty tuning (spec AE-3)

- Recorded at freeze: tasks dropped for ambiguity (after the second human review), tiers re-piloted and the knob used, tiers
  shipped outside their band at their measured rate.

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

## Not in this file

No task text, example, reference, family idea or canary (spec AE-3 contamination rules); those live only in the private store.
