# v0.19 design review — RLCD/Jev research against Axon v0.18

## Executive result

**Adopt as a targeted amendment, not a new subsystem.** Axon v0.18 already contained most of the right substrate: typed bounded decisions, dynamic candidate manifests, candidate order/set digests, score provenance, separate empirical correctness estimates, Brier/NLL calibration, dependency-aware same-state questions, listwise research, branch-aware batching and Neural Program learned-function boundaries. The research adds value mainly by making three areas explicit and testable: (1) a pairwise/multiway preference-training reference family, (2) calibration as an immutable deployment contract rather than a scalar, and (3) packed/fan-out and high-cardinality serving patterns with falsification tests.

## What v0.18 already had

- CX-05: Choice/BinaryProbability/OrdinalDistribution, dynamic candidate sets, score provenance and shared-state/speculative questions.
- CX-06: CalibrationArtifact, Brier/NLL/reliability, event-specific correctness and invalidation on semantic/input changes.
- CX-14: Outcome-Calibrated Decision Training research, independent/pointer/listwise arms and branch-isolated serving hypotheses.
- CX-26: typed composition plus Independent/ConditionallyRelevant/AnswerDependent scheduling semantics.
- CX-36/CX-23: source-bound Neural Program artifacts plus runtime/skill ownership.
- CX-37: DecisionResult already separates raw distribution/probability from `correctness{status,value,event_ref,domain_ref,estimator_ref,evidence_ref}`.

## Additions selected for v0.19

1. Bradley–Terry pairwise and Luce/Plackett–Luce multiway **reference** objectives.
2. A calibration registry key binding model/score/event/candidate/input/domain/calibrator semantics.
3. Proper-score and reliability release evidence; ECE is diagnostic, never sole gate.
4. Host/runtime `DECIDE/FANOUT/JOIN/CALIBRATION_GATE/VERIFY/ESCALATE` lowering without changing CX-36 r0.2.
5. Packed branch-isolated inference as a measured optional serving arm.
6. High-cardinality score/retrieve→shortlist→explicit-choice lineage.
7. Independent completion/authority despite calibrated confidence.
8. Production outcome drift/recalibration loop through existing shadow/admission governance.

## Explicit non-additions

- No claim that Zhang's packed-tree explanation is Jev's actual internal implementation.
- No ACE schema version bump solely for calibration; v1 already has a distinct correctness channel.
- No universal IIA requirement.
- No new authority path for Neural Programs, ACE providers or calibrated models.
- No assumption that specialized decision models beat current models; the bakeoff may reject them.

## Release consequence

This is large enough for **v0.19**, not a silent v0.18.1 patch: it adds normative gates/tasks and a release profile while deliberately preserving wire/artifact compatibility. All new product gates remain NOT_RUN.
