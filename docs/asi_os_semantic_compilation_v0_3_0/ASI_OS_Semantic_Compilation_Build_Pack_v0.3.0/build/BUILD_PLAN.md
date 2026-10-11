# Ordered Implementation Plan

## Implementation rule
A milestone is complete only for the exact enabled capability set and exact source revision tested. Every milestone keeps the previous gates green. The offline archive demonstrates contract mechanics; it does not count as P1–P5 delivery.

| Phase | Deliverables | Entry gate | Exit gate / evidence |
|---|---|---|---|
| P0 · Contract + owner baseline | Cortex/Forge owner map; repository integration map; boundary schemas; graph validation; synthetic fixtures; source provenance | Named contract owner; pinned Cortex/Forge inputs | No duplicate IntentIR/AIR/SIR/evidence/experiment/resource authority; invalid graphs/capabilities rejected; fixtures reproducible |
| P1 · One real vertical slice | CX-19 binding; Semantic Plan→CX-04 AIR lowering; optional Forge SIR bridge; isolated execution; CX-32 evidence projection; actual usage | Current owner map, approved task family and egress | Real task round-trips through canonical owners; independent check; denial/budget tests; lowering provenance retained |
| P2 · Evaluation + repair | Protected contract; optional typed judge; generative repair; exact-target evidence; immutable revision | P1 and owner-approved acceptance | Bad artifact rejected; bounded repair accepted only after reevaluation; injected judge state tested |
| P3 · Durable runtime | Durable journal, budget ledger, cancellation, scoped approvals, idempotency and reconciliation | P2; actual adapter guarantees reviewed | Crash matrix; no duplicate effects under tested windows; replay egress disabled |
| P4 · Controlled optimization | Axon adapter; repeated-trial IDs; pre-registration; baseline arms; holdout; canary/rollback | P3 or read-only equivalent; independent labels | Approved report with quality/economic evidence; no auto-promotion without signoff |
| P5 · Procedure promotion | Reusable plan templates; compatibility checks; tenant isolation; owner/version/rollback | Repeated cross-instance evidence | Reuse improves chosen metric on held-out task slice without relaxed criteria |

## P0: explicit repository discovery and owner binding
Start from `integration/CORTEX_V023_OWNER_PROFILE.json`, then inspect the live repository to confirm each binding still exists at the tested revision. Treat CX-19 IntentIR, CX-04 AIR, CX-03 authority, CX-13 isolation/effects, CX-21 evaluation, CX-11/CX-18 admission, CX-29 strategy, CX-32 evidence, CX-33 experiments, CX-34 resources, CX-16/MiCode workflow, and Forge SIR/Transform/Migration/BuildProgram as canonical owners unless the repository itself says otherwise. Extend rather than duplicate them. Record any BFlow agent-only policy amendment. Port fixture checks into the repository's native language only when that improves integration; keep portable boundary fixtures shared.

Before P1, implement four conformance seams: (1) approved IntentIR reference → compilation record; (2) Semantic Plan → AIR lowering/round-trip; (3) software nodes → Forge SIR/Transform/Migration binding when supported; (4) local outcome → canonical CX-32 evidence and CX-33 experiment references. Resource projections are advisory snapshots only and must never mint CX-03/CX-34 authority.

## P1: minimal scope
Choose **one** task family. Prefer local repository test repair with no push/deploy, or document transformation with no external publishing. Use one compiler model and one isolated execution path. Exact typed inputs/outputs and deterministic acceptance matter more than sophisticated decomposition. Runtime and evaluator failures must surface as failures, not malformed success artifacts.

## P2: evaluation before optimization
First prove rejection of an intentionally flawed output and successful reevaluation of a repaired exact artifact. Introduce a typed judge as supplementary evidence, not as the sole truth source. Use a separate generative repair capability. Count all calls. Validate calibration on task-specific labels before learned auto-acceptance; otherwise abstain or keep deterministic/human acceptance.

## P3: effects are a separate release
Do not enable real external messaging, payments or deployment simply because code execution works. Add one low-risk effect only after its approval, idempotency, ambiguous-timeout and recovery tests pass. Runtime operations during outages remain auditable and fail closed.

## P4: test the thesis
Register compiler/evaluator/repair/routing variants as CX-29 StrategyArtifacts and trials under CX-33 rather than creating a local experiment namespace. Apply hard correctness/security/authority gates before Pareto or utility ranking. Compare interpreted and compiled arms with fixed evaluation, task splits and enough power for the chosen quality claim. Include compilation and judge overhead, cold versus warm starts, failed tasks and abstentions. Let negative results select a narrower compilation boundary rather than rewrite the scorecard. Keep active strategy rollback independent of optimizer availability.

## P5: reuse without context leakage
Promote a parameterized plan with explicit applicability constraints; do not memorize a task-specific trace and call it a general skill. Test schema drift, tenant mismatch, stale tools, changed policies, new model versions and hidden data leakage. Dynamic inference remains explicit where the task still needs judgment.

## Suggested ownership
A runtime/security owner handles admission and effects; a compiler owner handles lowering/repair; an evaluation owner controls criteria and holdouts; an optimization owner handles trials and promotion evidence. One person can fill several roles during a prototype, but change approval and the final yardstick must not be silently owned by the generating agent.

No schedule or headcount estimate is asserted. Estimate after repository discovery and the first real vertical slice reveal integration constraints.
