# Semantic Working-Set Manager build guide

This guide implements CX-28 as managed cognitive memory rather than generic summarization.

## Build order

1. Inventory context artifacts and assign authority/privacy/recompute metadata.
2. Implement hard pinning before any learned relevance scoring.
3. Add dynamic rule/skill/map selection over authorized candidates.
4. Add tool-call/result semantic GC with `PIN | KEEP_VERBATIM | KEEP_STRUCTURE | COMPRESS | DROP_RECOMPUTABLE`.
5. Implement recompute contracts and durable retrieval references.
6. Bind every decision to intent/state/catalog digests and reject stale async results.
7. Emit `WorkingSetReceipt` for every protected learned call.
8. Add cache-aware model routing and semantic-query planner experiments.
9. Benchmark long-horizon quality/context/cost against full-context and generic-summary controls.

## Semantic query planner rule

Prefer deterministic filters and authorization checks before learned predicates. Use lexical/index narrowing where recall permits, then semantic scoring/ranking, then generation/reasoning only for unresolved cases.

## Cognitive cascade

Instrument the ordered strategy ladder so each fallback records why the cheaper representation failed or abstained. The cascade is a runtime policy, not permission to bypass verifier or capability gates.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
