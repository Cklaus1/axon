# Changelog v0.13

## Reflexive self-application

- Added **CX-29 — Reflexive Self-Application Plane**.
- Added `build/SELF_APPLICATION.md`.
- Added `CognitiveOperationRecord`, `ImprovementIntent`, `SelfApplicationRun` and `PrimitiveProposal` protocol concepts.
- Added B116–B127 covering immediate instrumentation, frozen replay, no-effect shadowing, low-risk canary/rollback, internal specialization and research-only recursive optimization.
- Added fourteen G29 gates covering observability, protected-kernel boundaries, independent evaluation, lineage, no metric gaming, self-hosting, rollback, data separation and new-primitive admission.
- Defined the rollout order: observe now → replay → shadow → protected low-risk canary → later higher-impact optimization.
- Explicitly made Working-Set Manager, Supervisor, Model Router, semantic selection, composition and specialization policies optimization targets rather than permanently hand-written infrastructure.
- Preserved capability/effect enforcement, interpreter semantics, locked evaluation/admission ownership, verifier authority, provenance and rollback as protected roots of trust.
