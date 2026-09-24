# Outcome-Calibrated Decision Models — v0.19 build guide

## Purpose

Turn the useful, falsifiable parts of the 2026-09-21 RLCD/Jev research into Axon build work without encoding an unverified description of Jev internals. This guide is subordinate to CX-05, CX-06, CX-14, CX-23, CX-26 and CX-37.

## Architecture

```text
immutable state/effective input
        │
        ├─ DECIDE<T> ───────────────┐
        ├─ FANOUT {Decision<T>...}  │
        │                           ▼
        │                    typed distributions
        │                           │
        │                    calibration lookup
        │                           │
        │                 correctness estimate or unavailable
        │                           │
        └──────────────────── policy/risk gate
                                    │
                    ┌───────────────┼────────────────┐
                 execute          verify          escalate/reason
                    │               │                 │
                    └──────────── outcome/evidence ───┘
                                      │
                                offline drift/retrain
```

## Inner build loop

1. Freeze one decision event: observation/effective input, question wording/semantic ID, candidate construction/order policy, correctness label and consequence class.
2. Add negative fixtures first: missing candidate, wrong-order digest, stale calibration key, unsupported score source, partial fan-out, candidate omitted by shortlist.
3. Run the incumbent typed backend as the reference path.
4. Add one challenger only: pairwise objective, multiway/listwise arm, post-hoc calibrator, packed serving path or high-cardinality shortlister.
5. Compare on identical frozen examples with proper scores and task outcomes; for serving optimizations also compare numerical/decision equivalence.
6. Record limitations and leave activation disabled unless the existing admission path accepts it.

## Outer loop

Run a dependency-closed release slice through real ACE/MiCode consumers where available. Measure end-to-end latency, quality, abstention, verifier load, cost, data egress and effect outcomes—not just model-call speed. Exercise cancellation, partial failure, stale calibration, wrong-domain calibration, unsupported shared state, shortlist miss and next-turn recovery.

## Meta/adversarial loop

Ask each cycle:

- Are we measuring the same semantic event across backends?
- Did candidate construction/order or working-set projection change and invalidate calibration?
- Did a score/probability acquire a stronger name without new evidence?
- Did batching/packing change outputs through sibling leakage or positional artifacts?
- Did a shortlister hide the correct option and make the final chooser look artificially accurate?
- Did a high probability bypass PermissionGate, verification or completion criteria?
- Did production feedback change an active model/calibrator without a new admitted version?
- Are Jev/TypeSafe claims clearly separated from Zhang's hypotheses and Axon measurements?

## Training/evaluation matrix

Compare at least: incumbent constrained generation; direct/sequence scoring where supported; Bradley–Terry pairwise arm; Luce/Plackett–Luce multiway arm; optional listwise interaction arm; and post-hoc calibration variants. Report ranking/decision quality separately from calibration quality. Use Brier/NLL plus reliability and selective-risk views; retain finite-sample support.

## Packed serving arm

Packed execution is optional. A candidate implementation shares immutable prefix/state while masking branches so sibling candidate/question tokens cannot be attended to. Require packed-vs-separate equivalence, contamination probes, position-reset probes, order permutations, peak memory, latency and throughput. Reject the optimization if it saves time but changes semantics outside declared tolerance.

## High-cardinality arm

```text
full candidate universe U
→ deterministic/recorded retrieval or independent scoring
→ shortlist S + policy/digests + recall evidence
→ explicit joint choice over S
→ optional calibrated correctness for the declared S-conditioned event
```

Never renormalize stage-1 scores and call them `P(choice | U)` unless a separately validated model actually defines that event. Track omission risk and the cost/quality frontier as shortlist size changes.

## Acceptance boundary

Reference-tool/unit-test PASS proves only that the mathematics and package contracts are implemented as specified. Product gates remain `NOT_RUN` until a real Axon producer/consumer/outcome population supplies evidence.
