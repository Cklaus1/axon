# Changelog v0.11 — decision composition and semantic alignment

Prepared 2026-09-19. Documentation/build-plan update only; no Axon product/model gate was executed.

## Added

- **CX-26 — Decision Composition Runtime**
  - runtime-owned typed artifact catalogs;
  - SELECT / PROJECT / COPY for authoritative existing values;
  - bounded COMPOSE for DAGs/specs/plans/pipelines;
  - explicit partial/absence/authority/fallback states;
  - Intent IR clause traceability;
  - dependency-aware composition semantics;
  - generation only for unresolved novel content.
- `build/DECISION_COMPOSITION.md`.
- `build/SEMANTIC_ALIGNMENT_LOOP.md`.
- B94–B101 work packages.
- G26-* composition gates and new G20 semantic-alignment gates.
- v0.11 protocol records for artifact catalogs, projection receipts, composition plans, semantic-definition revisions and active-label records.

## Changed

- CX-20 now treats question wording, criteria, state projection, decomposition and candidate policy as versioned research artifacts distinct from model weights.
- CX-22 may specialize recurring cognition into a guarded `CompositionPolicy`.
- CX-19 may lower approved Intent IR into known task/evidence artifacts before invoking GENERATE for novel slots.
- BUILD_PLAN, TASKS, ACCEPTANCE_GATES, REVIEW, DECISIONS, GLOSSARY, SOURCES and STATUS now reflect constrained selection/composition and semantic-definition optimization.

## Research motivation

Recent browser/action, extraction, semantic-alignment, tool-gating and constrained-rendering projects converge on a broader pattern: when the world already contains the relevant objects or verified building blocks, intelligence can select and compose them instead of regenerating them. This package treats those projects as design inputs only; their benchmark claims are not Axon evidence without CX-20/CX-21 reproduction.

## Current package counts

- 27 Draft specs
- 102 Not-started work packages
- 176 proposed product gates
- 75 Markdown files
- 0 product/model gates executed
