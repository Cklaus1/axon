# Changelog v0.5 — intent-first Cortex integration

Prepared 2026-09-18. Documentation/build-plan update only; no Axon product gates executed.

## Added

- **CX-19 — Intent compiler, typed Intent IR and semantic approval.** Natural language/system proposals now lower first to a typed desired-outcome contract, not directly to executable AIR/`.ax`.
- **build/INTENT_COMPILER.md** with vertical slices, negative cases and inner/outer/meta loops.
- Intent/evidence lineage protocols and gates: `G19-parse`, `G19-ambiguity`, `G19-authority`, `G19-evidence`, `G19-render`, `G19-trace`.
- Work packages **B52–B57** for source mapping, Intent IR, ambiguity resolution, semantic approval, AIR lowering, and self-improvement-through-intent.

## Changed

- Cortex is explicitly **intent-first**: `Natural intent → Intent IR → AIR → Axon program/actions → evidence graph`.
- Human and system-generated self-improvement requests share one Intent IR and authority/admission path.
- `Done` semantics bind to the approved Intent IR acceptance/evidence clauses; replanning may change method but cannot weaken success criteria.
- Semantic review becomes the primary human UX over the typed contract; raw AST remains inspectable but is no longer the only meaningful approval representation.
- M1 now includes the basic intent contract/lowering slice; advanced self-generated improvement intent is staged after admission/crystallization is available.

## Not claimed

No natural-language parser quality, intent compilation success rate, UI, new syntax, or self-improvement behavior was implemented or measured by producing this package.
