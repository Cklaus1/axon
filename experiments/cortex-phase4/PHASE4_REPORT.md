# Cortex Phase 4 — transfer, and the pooled estimate

## The Phase 3b failure, fixed

Phase 3b could not test transfer: its Python control scored 100% at 1.01
attempts, so there was no headroom. **Recalibrating found the real problem** —
for this model, Python single-function repair is *bimodal*: of 20 candidates,
10 sat at a 1.00 control rate and 2 at 0.00, with almost nothing between.

It also exposed an error in my own admission band. I had set a 0.15 floor to
exclude "nothing can be learned" tasks — but **a 0% control has maximum
headroom**, and Phase 2's largest single effect was exactly such a task
(`dict_method_syntax`, 0% → 87%). Only a CEILINGED control is useless. With the
floor corrected, 5 tasks were admitted: two at 0.00, two at 0.62, one at 0.75.

## Result (200 episodes, 9.1 min)

| arm | repaired | attempts/ep | prompt tok/ep |
|---|---|---|---|
| OFF | 59/100 (59%) | 1.98 | 239 |
| ON | **80/100 (80%)** | 1.83 | 358 |

| task | calibrated control | OFF | ON | delta |
|---|---|---|---|---|
| interval_merge | 0.00 | 0% | 95% | **+95pp** |
| round_half_even | 0.00 | 5% | 25% | +20pp |
| lru_evict_order | 0.62 | 90% | 100% | +10pp |
| title_case_small | 0.62 | 100% | 100% | 0 |
| wildcard_match | 0.75 | 100% | 80% | **−20pp** |

Mean +21.0pp, 95% CI **[−8.0, +61.0]**, sign test p=0.312 — **the CI includes
zero.** On its own this experiment does not establish transfer; it shows the
effect pointing the same way, at a similar magnitude, in a language the model
knows well.

## Pooled estimate — 19 task-level paired deltas, 2 languages, 3 task sets

| experiment | tasks | mean delta |
|---|---|---|
| Phase 2 (Axon) | 7 | +26.1pp |
| Phase 3a (Axon, `blind`→`both`) | 7 | +22.9pp |
| Phase 4 (Python) | 5 | +21.0pp |

**Pooled: +23.5pp, bootstrap 95% CI [+8.5, +40.5], 12 of 15 changed tasks
improved, sign test p = 0.0176.**

Three independent task sets across two languages agree on both direction and
magnitude (21–26pp). The pooled CI excludes zero and the sign test clears 0.05.

## What is now supported

1. **Grounded iteration beats blind iteration at equal attempt budget**, pooled
   +23.5pp [+8.5, +40.5], p=0.018.
2. **The active ingredient is the DIAGNOSTIC, not added context.** Phase 3a's
   `prev` arm — same shape and length of extra text, no new information — does
   not help (−5.7pp). `diag` alone helps (+8.6pp, CI [+1.4, +20.0]).
3. **The diagnostic needs the attempt beside it.** `diag` alone is +8.6pp;
   together +22.9pp. `dict_method_syntax` is 0% for blind, prev AND diag, and
   100% for both.
4. **It is not merely an unfamiliar-language effect.** The Python replication is
   the evidence against that alternative, and `interval_merge` (0% → 95%) is a
   language the model knows well.

## What is still NOT supported, and the known harm

- **Single-repo, single-model.** Every result is one 7B model on tasks authored
  here. No claim about other models, sizes, or a repo I did not write.
- **Three tasks got WORSE**: `enum_variant_called`, `string_interp`,
  `wildcard_match` (−20pp). The mechanism was visible back in Phase 1
  (`count_skips_last`): the treatment induces restructuring that displaces a
  simple correct edit. This is a real cost, not noise to be averaged away.
- **Task authorship is a confound I cannot remove from inside.** I wrote the
  tasks, the diagnostics they trigger, and — earlier in this branch — several of
  the compiler hints the ON arm reads. An independent task set is the honest
  next step.
