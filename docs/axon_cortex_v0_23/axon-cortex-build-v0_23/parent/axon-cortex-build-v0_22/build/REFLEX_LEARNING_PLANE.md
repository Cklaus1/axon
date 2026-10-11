# Reflex Learning Plane build guide

Implements CX-25. Separate candidate learning from active serving so the system can train aggressively without allowing training code to self-promote.

## Inner loop

1. Freeze eligible records and exact training objective.
2. Train one candidate in an isolated learner environment.
3. Content-address all outputs and conversion artifacts.
4. Load candidate into a shadow sampler.
5. Replay the same protected suite used by the incumbent.
6. Export CX-20/CX-21 evidence; stop with reject/inconclusive or submit to CX-11.

## Outer loop

Operate many learner experiments against a shared immutable corpus/suite registry while keeping the active sampler stable. Maintain parent/child model lineage and transfer/calibration results.

## Meta loop

Evaluate whether asynchronous training, mixed-policy rollouts or faster weight transport actually improve time-to-admitted-capability. Do not optimize weight publication latency if evaluation/admission remains the bottleneck.

## First implementation

Use file/object-store candidate publication, not direct GPU streaming. Prove learner/sampler authority separation and exact artifact identity first. Faster NCCL/GPU transport is an optional later optimization.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
