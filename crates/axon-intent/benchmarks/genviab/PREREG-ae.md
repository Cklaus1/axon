# PREREG: authoring-eval (`ae`) - pre-registration

**Status: DRAFT (skeleton; the integrator freezes it after the pilot, spec AE-3 "Pilot, then freeze")**

`ae.py run` refuses every non-`--pilot` run until the status line reads `**Status: FROZEN ...**` and the pins below are filled.
The pins are read verbatim (`- key: \`value\``); a run whose manifest sha or served model series differs is refused (spec AE-4).

## Pins (filled at freeze)

- set version: `<ae-1.0>`
- A/A null SD: `<headline FT-pass Δ SD of the pilot's A/A run>` (the weekly screen's rerun trigger, |Δ| ≥ 2.5 × this)
- manifest sha256: `<sha256 of <private>/ae-1/manifest.json>`
- model series: `<cere:Qwen3.8-Flash-Next:<models-entry sha16>>` (genviab `http_series`)

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

## Not in this file

No task text, example, reference, family idea or canary (spec AE-3 contamination rules); those live only in the private store.
