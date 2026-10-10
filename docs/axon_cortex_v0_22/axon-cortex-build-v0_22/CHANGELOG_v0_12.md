# Changelog v0.12

Prepared 2026-09-19. Documentation/specification update only; no Axon product gate was executed.

## Added

- CX-27 Semantic Supervisor Plane.
- CX-28 Semantic Working-Set Manager.
- `build/SEMANTIC_SUPERVISOR_PLANE.md`.
- `build/SEMANTIC_WORKING_SET_MANAGER.md`.
- B102–B115 work packages.
- G27/G28 acceptance gates.
- supervisor, working-set, recompute and cognitive-cascade protocol records.

## Integrated research consequences

- independent supervisor around coding/agent workers;
- deterministic intervention policy with steer/verify/hold/stop/escalate/propose-finish;
- semantic context GC and protected pinning;
- dynamic authorized rule/skill/map selection;
- stale async context-decision rejection;
- effective working-set receipts;
- cache-aware model routing;
- semantic predicates as query-planner operations;
- observable cognitive cascades from deterministic logic to reasoning;
- MiCode v0.5 supervisor/context records as new experience-plane inputs.

## Unchanged boundaries

- learned supervision does not grant authority;
- working-set selection cannot drop protected intent/authority/acceptance/verifier state;
- `ready_to_finish` never equals `VerifiedComplete`;
- external repository claims remain research leads until reproduced under CX-20/CX-21.
