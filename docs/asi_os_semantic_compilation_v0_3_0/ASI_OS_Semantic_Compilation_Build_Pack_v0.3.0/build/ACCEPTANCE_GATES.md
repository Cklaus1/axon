# Acceptance Gates

## G0 — Archive coherence (executed for this package)
Schemas pass their metaschema checks. Local references resolve without network. Valid fixtures pass structural/selected semantic checks. Invalid fixtures fail with expected diagnostic codes. Offline tests pass. Included documents and local links exist. Inventory and hashes are consistent. Simulator clearly labels synthetic evidence. See `reports/VALIDATION.md` for the actual results.

G0 proves a coherent build pack, **not** a secure/working production runtime.

## G0A — Cortex/Forge owner alignment (executed for this package)
The Cortex v0.23 and Forge v0.3 inputs are hash-pinned. The owner profile creates no new authority, portable Semantic Plan objects are projections rather than a second AIR runtime, semantic operation is distinct from execution mechanism, Forge remains owner of SIR/program semantics, and evidence/experiment/resource records are projections to CX-32/CX-33/CX-34. Requirement crosswalk coverage and input-lock consistency pass offline tests.

G0A proves **contract alignment only**. It does not prove that the live Cortex services accept these projections or that a real AIR/SIR round-trip preserves semantics; those remain G1/P1 integration evidence.

## G1 — Real vertical slice
Approved intent -> real compiler -> validated plan -> isolated execution -> independent acceptance -> exact artifact/trace. Show both success and deliberate failure. No unapproved external effects. Every provider call is metered or explicitly marked unknown. A blocked policy decision prevents dispatch.

## G2 — Evaluation and targeted repair
Protected criteria cannot be edited by generation or repair. A bad artifact fails. A permitted repair creates a new revision, invalidates affected descendants and passes checks tied to the new exact output. Missing evaluator/evidence causes abstention or error. Repair/budget caps stop loops. Typed judge probabilities, scores and confidence remain distinct.

## G3 — Security and recovery
Real adapter tests cover credential leakage, scoped egress, approval changes/expiry, unsafe retries, budget reservation races, crash windows, ambiguous effects, cancellation and sandbox descendants. Captured replay cannot reach live effectful adapters. No security claim is based only on schema/linter checks.

## G4 — Optimization evidence
The same task/arm can run in several distinct trials with unique run IDs. Pre-registration and holdouts are frozen. Required quality evidence passes the approved margin/statistical test. Costs include all overhead and failures. Promotion is signed off separately and can be rolled back. Synthetic fixture rows are rejected from production promotion.

## G5 — Reusable procedures
Cross-instance holdout success, compatibility checks, tenant isolation, versioned ownership and rollback. No stale permissions or private context travel with a reused template. Task-class generalization is supported by data rather than asserted from a single trace.

## Release interpretation
Enable only the capabilities covered by passed gates. A read-only evaluation prototype can ship internally with G0–G2 scope while financial/messaging effects remain disabled. Do not label unexecuted G3–G5 tests “not applicable” merely to claim the entire OS is complete.
