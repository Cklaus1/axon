# Architecture and Trust Boundaries

## Canonical-owner rule
Under the Cortex v0.23 profile, Semantic Compilation is a composition layer. CX-19 owns approved intent semantics; CX-04 owns AIR execution semantics; Forge owns software SIR; CX-03/CX-13 own authority/effects; CX-21 owns protected coding evaluation; CX-32/CX-33/CX-34 own evidence/experiments/resources. Local plan, registry, evidence and resource documents are projections or portable fixtures, not alternate global authorities.


## Control plane versus execution plane
Start as a modular service or library in the existing repository, not as seven new microservices. Separate the interfaces and authority domains first. Split deployment only when isolation, scaling or ownership demands it.

The control plane authenticates principals, resolves registry/policy snapshots, persists plans and runs, reserves budgets, enforces authorization, schedules work, aggregates evidence and controls release. Compiler, model, evaluator and repair workers run with scoped inputs and no ambient action credentials. Execution adapters are the only path to external capabilities.

## Component contracts
| Interface | Input | Output | Persistent owner |
|---|---|---|---|
| Intent admission | User request + authenticated context | Normalized intent or clarification | Intent store |
| Compile | Approved intent + scoped registry/context | Candidate bundle + diagnostics | Plan store |
| Validate | Plan + registry + evaluation contract | Findings; never a grant | Admission service |
| Rank | Eligible candidates + allowed features | Selection/abstention + policy revision | Axon adapter |
| Dispatch | Admitted invocation + exact args | Result/effect status + metering | Runtime/effect ledger |
| Evaluate | Artifact + protected criterion + evidence | Typed result | Evidence store |
| Repair | Failed target + protected contract + limits | New candidate revision | Plan/artifact store |
| Release | Exact artifact + complete evidence | Release receipt or refusal | Release service |

## End-to-end transaction boundary
The compiler writes a candidate. Validation resolves exact dependency locks. The selector may choose an eligible candidate but cannot change its contents. Admission freezes bindings/config and reserves an execution envelope. Runtime dispatches each node under current policy. Work completion triggers evaluation. Failed required checks block release. Repair creates a new candidate revision linked to the previous one; it does not mutate the execution record. Release binds the exact output digest to the passing evidence set.

## Work plan plus evaluation contract
The work graph contains the computation. The evaluation contract sits outside that graph so a compiler cannot silently delete the final checker. Optional inline `evaluate`/`assert` nodes can stop work earlier, but they do not replace release evaluation.

Compiler-proposed checks enter as suggestions. An owner-controlled contract selects mandatory checks and a decision policy. A checker capability's implementation and detailed schemas are pinned in the registry. One model can perform multiple roles during experimentation, but role separation alone does not confer independence; record correlated provider/model lineage.

## Storage boundaries
Keep intent/plan metadata, run event journal, effect ledger, artifact store, evaluation evidence, policy receipts and experiment records logically separate. Tenant scope accompanies every key. An artifact reference is checked at read time, not treated as a globally readable hash. Logs default to content references and redacted summaries, not full prompt/credential capture.

## Failure containment
Compiler failure returns diagnostics. Evaluator failure produces `error`, not pass. Optimizer unavailability falls back only to a preapproved baseline. Policy unavailability blocks dispatch. Artifact-store unavailability blocks evidence-dependent release. Unknown external effects pause reconciliation. Budget exhaustion stops new calls and preserves an explainable partial outcome.

## First deployment
Single control-plane process, durable local database in development, content-addressed local artifact storage with scoped access, one sandbox adapter and one provider adapter are enough for the first real slice. Database and sandbox products are intentionally not selected here. Their choice belongs to repository discovery and the security review.
