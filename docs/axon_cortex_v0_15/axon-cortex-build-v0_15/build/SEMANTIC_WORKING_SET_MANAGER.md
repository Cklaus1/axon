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
