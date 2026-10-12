# ACE provider runtime and transport build guide

ACE means **Axon Cognitive Execution**. AIR defines semantics; CX-34 selects an eligible capability; CX-37 binds an immutable execution plan and invokes an ACE provider; CX-03 remains the only application-world effect authority.

## Build order

1. Migrate names and schema IDs while retaining read-only legacy ANEA aliases.
2. Implement `ACEExecutionPlan` and dispatch revalidation before adding transports.
3. Implement one in-process provider against the general result union.
4. Add streaming/finalization and effect-boundary tests.
5. Add DataHandlingProfile, isolation and determinism receipts.
6. Add durable handles/reconciliation.
7. Add one local/remote transport adapter and compare the bounded logical result byte-for-byte where the contract permits deterministic encoding.
8. Only then run the real MiCode producer/consumer slice.

No model training, latent/KV compiler or world-model transition ABI is required for ACE v1. Native state remains opaque unless a provider separately advertises and proves it.

## v0.19 calibrated decision consumption

ACE keeps the v1 schema. `distribution`/`probability` remains a decision-event output; `correctness` is the separate calibrated correctness channel and is `Estimated` only with matching event/domain/estimator/evidence references. Providers may batch/pack internally but must preserve declared feature/equivalence semantics. High-cardinality staging and fan-out are represented through existing typed requests/results plus linked runtime/evidence records rather than a wire-format fork.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
