# Architecture Decision Records

## ADR-001 — Intent is desired behavior; releases retain implementations
**Decision:** Retain approved intent, constraints and acceptance as source of desired behavior, while preserving exact generated artifacts and provenance for each release. **Reason:** Recompilation is not guaranteed to regenerate identical code. **Rejected:** Discard every generated implementation as soon as an intent is stored.

## ADR-002 — Bounded DAG first
**Decision:** Keep a bounded DAG reference profile and reject general control flow until effect/recovery semantics are explicit. **Reason:** Branch joins, loops, recursion and cancellation need explicit effect/recovery semantics. **Consequence:** Some tasks return unsupported or use bounded supervisory recompilation until a future contract supports them.

## ADR-003 — Evaluation is not repair
**Decision:** Typed decision adapter, evidence aggregation, generative diagnosis and repair are separate capabilities. **Reason:** Jev's documented typed outputs do not imply prose/code generation [S03]. **Consequence:** Repair has independent usage, privileges and tests.

## ADR-004 — Protected external acceptance contract
**Decision:** Required criteria live outside the compiler-controlled graph. **Reason:** A generated plan must not certify itself by omitting its hardest check. **Consequence:** Contract updates need review and new identity; generated extra tests remain useful but insufficient for promotion.

## ADR-005 — Runtime owns authority
**Decision:** Dispatch checks exact arguments under current policy; compiler/evaluator/optimizer cannot authorize effects. **Reason:** A plan can be syntactically valid and still unauthorized. **Consequence:** Approval broker and effect ledger are release blockers for real writes.

## ADR-006 — Immutable repair and optimization
**Decision:** Rewrites create new revisions and invalidate affected descendants. **Reason:** In-place changes break provenance and approvals. **Consequence:** A bounded supervisor handles deoptimization; active graph mutation is deferred.

## ADR-007 — Record-only optimization at bootstrap
**Decision:** Exploration and auto-promotion disabled initially. **Reason:** There are no task-specific comparative data in this package. **Consequence:** Establish independent outcomes and trial identity before adaptive policies.

## ADR-008 — No mandatory vendor or new infrastructure stack
**Decision:** Provider-neutral adapters; use the existing repository's scheduler/config/telemetry when compatible. **Reason:** Current Axon/MiCode code was not available for audit. **Consequence:** Integration discovery precedes code changes and product selection.

## ADR-009 — Narrow package canonical JSON profile
**Decision:** Use an explicit integer-only canonical identity profile for fixture source documents. **Reason:** Avoid pretending Python JSON dumping is a universal cross-language canonicalization standard. **Consequence:** Production evidence with floating-point numbers needs exact-byte hashing or a separately defined numeric profile.

## ADR-010 — BFlow policy amendment is explicit
**Decision:** Propose deterministic runtime execution while preserving intent-first authoring. **Reason:** The supplied BFlow agent-only policy conflicts with executing business logic deterministically. **Consequence:** Owner approval and an explicit source-policy update are required rather than silently reinterpreting that document.


## ADR-011 — Cortex is the canonical owner profile
**Decision:** In Cortex v0.23 mode, bind to CX-19 IntentIR, CX-04 AIR, CX-03/CX-13 authority/runtime, CX-21 evaluation, CX-32 evidence, CX-33 experiments and CX-34 resources. **Reason:** Avoid parallel authorities and incompatible learning/evidence histories.

## ADR-012 — Separate semantic operation from mechanism
**Decision:** Every executable capability-backed node declares both semantic meaning and physical mechanism. **Reason:** Model/tool/code are implementation choices, not task semantics. **Consequence:** Optimizers can substitute implementations only while preserving the semantic contract.

## ADR-013 — Forge owns program semantics
**Decision:** Use Forge v0.3 SIR/Transform/Migration/BuildProgram rather than inventing a second software-semantic IR. **Reason:** Cortex v0.23 already pins Forge as the program-semantic substrate.

## ADR-014 — Project evidence, experiments and resources
**Decision:** Semantic Compilation emits projection payloads into CX-32/CX-33/CX-34 rather than owning global graphs/registries. **Reason:** Local convenience state must not become authority or promotion evidence by accident.
