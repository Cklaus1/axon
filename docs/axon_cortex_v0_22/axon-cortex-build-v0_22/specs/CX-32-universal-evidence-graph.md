---
id: CX-32
title: "Universal Evidence Graph: typed provenance from intent to verified claims"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-01", "CX-02", "CX-04", "CX-10", "CX-11", "CX-19", "CX-29", "CX-30"]
first_stage: M1
implementation_evidence: []
---

# CX-32 — Universal Evidence Graph

## 1. Purpose

Unify the growing set of Axon receipts, traces, verifier outputs, project experiments, supervision records and learning lineage into one typed evidence graph. The graph is the canonical substrate for audit, replay, debugging, admission, self-improvement and cross-project transfer.

The core chain is:

```text
Intent / ImprovementIntent
→ Observation / WorldState
→ CognitiveOperation
→ Decision / Composition / Plan
→ AuthorizedAction
→ WorldChange
→ EvidenceArtifact
→ Claim
→ Verification
→ Admission / Promotion / Rollback
```

A claim is never supported merely because a model emitted it. Each consequential claim points to the concrete evidence nodes, transformations and verifier that justify it.

## 2. Graph model

Every node has immutable identity, schema version, producer revision, timestamp/sequence, project/tenant scope and content digest. Edges are typed, directional and provenance-preserving. At minimum the graph supports:

- `DERIVED_FROM`
- `OBSERVED_IN`
- `DECIDED_FROM`
- `AUTHORIZED_BY`
- `EXECUTED_AS`
- `CHANGED`
- `SUPPORTS`
- `CONTRADICTS`
- `VERIFIED_BY`
- `SUPERSEDES`
- `INVALIDATED_BY`
- `PROMOTED_AS`
- `ROLLED_BACK_TO`

Raw events may remain in append-only stores; the evidence graph indexes stable semantic relationships without rewriting source artifacts.

## 3. Claim and evidence typing

Claims declare scope and strength, for example:

```text
ObservedFact
DerivedFact
StatisticalEstimate
CounterfactualEstimate
VerifiedProperty
ProofBackedProperty
TransferClaim
```

The graph does not coerce statistical confidence into proof. `VerifiedProperty` requires the protected verifier/evidence contract that produced it. Counterfactual and simulated evidence remain visibly distinct from realized outcomes.

## 4. Negative and contradictory evidence

Contradictions are retained as first-class edges. Promotion systems query both supporting and contradicting evidence. A newer success does not erase a prior failure, and de-generalization in CX-31 is represented by applicability revisions linked to the contradictory episodes that motivated them.

## 5. Effective-input closure

For any protected model-assisted result, the graph must be able to recover or reference:

- exact effective working set;
- state/snapshot digest;
- candidate catalog/order;
- model/backend artifact identity;
- semantic definition revision;
- calibration/probability provenance;
- authority and budget context;
- downstream outcome and verifier evidence.

This creates a common lineage layer across Reflex, supervisor, working-set, repository optimization and self-application.

## 6. Evidence queries

The graph supports bounded queries such as:

- Why was this action permitted?
- Which observations support this claim?
- Which component revision produced this decision?
- What evidence contradicted this abstraction?
- Which repositories contributed to this shared capability?
- What changed between incumbent and challenger?
- Which claims depend on a quarantined source?

Query execution must preserve authorization and data-use boundaries; evidence visibility does not imply authority to execute or disclose source content.

## 7. Retention and invalidation

Evidence can be archived or compacted, but durable graph identity and invalidation history remain. If a source is revoked, corrupted or disallowed, dependent claims become quarantined/needs-reevaluation rather than silently disappearing.

## 8. Integration

- CX-10 writes replay/episode lineage into the graph.
- CX-11 consumes evidence subgraphs for admission.
- CX-19 binds approved intent clauses to later evidence.
- CX-21 benchmark outcomes become protected evidence nodes.
- CX-29 self-application runs form challenger/evaluation/promotion subgraphs.
- CX-30/CX-31 add repository/project-family transfer evidence.
- CX-27/CX-28 add supervisor and effective-working-set receipts.

## 9. Acceptance gates

**G32-node-identity:** every protected evidence node is content- or transaction-addressed with immutable producer/schema identity; mutable aliases cannot substitute for evidence identity.

**G32-typed-edges:** graph relationships use a closed/versioned edge vocabulary and reject invalid source/target type combinations rather than storing ambiguous free-form provenance.

**G32-claim-strength:** claim types preserve observation/statistical/counterfactual/verified/proof distinctions; weaker evidence cannot be relabeled as stronger evidence by downstream consumers.

**G32-contradiction:** contradictory/negative evidence is retained and queryable; promotion/admission queries cannot ignore known contradictions without an explicit scoped rationale.

**G32-effective-input:** protected learned decisions link to the exact effective-input/candidate/model/definition/authority receipts needed for replay or an explicit reason exact reconstruction is impossible.

**G32-admission-closure:** every promoted artifact has a traversable evidence subgraph from approved intent/hypothesis through experiment and protected verification to the admission decision.

**G32-invalidation:** source revocation/corruption/data-use changes propagate quarantine or reevaluation state to dependent claims/artifacts without deleting lineage.

**G32-access-control:** graph query/export obeys project/tenant/privacy/data-use policy and cannot use provenance visibility as a route around capability or disclosure controls.

## 10. First deliverable

Build one end-to-end evidence subgraph for a resettable MiCode/Axon repair episode, including intent, observation, Reflex/working-set decisions, action, test evidence, completion verification and one replay/challenger comparison. Demonstrate a contradiction edge and an invalidation propagation fixture.

## v0.16 review amendment — Consistent evidence closure

The graph indexes CX-10 artifacts; it is not another mutable truth store. Derivation dependencies must be acyclic, while contradiction and other relations may form non-derivation cycles. Admission evaluates a versioned closure snapshot including required evidence, known contradictions and current scoped invalidations. Hidden/withheld/missing evidence is incomplete, never positive evidence of absence. Source revocation propagates along relevant typed derivation dependencies, not every graph edge. Retention obeys CX-10 and preserves permitted tombstones only.

**G32-closure-snapshot:** a source invalidation or withheld mandatory node during admission blocks stale closure use; contradiction cycles do not break traversal, and unrelated artifacts are not invalidated solely by co-membership in the graph.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE physical execution evidence

P08/P10 enrich existing graph nodes and source journals with immutable mapping/profile, exact input, physical attempts, normalization/repair/fallback, observed versus accepted producer and unknown costs. Never merge inference events with executed tool actions or estimated outcomes with observations.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G32-ace-attempt-lineage:** Every physical attempt/transformation and final accepted producer links to existing effective input, operation, budget and evidence; missing/failed/speculative work remains visible and negative or invalidated evidence is not discarded.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G32-r22-sidecar-bindings:** Import validates task/arm/trial/attempt/operation, principal, policy, effective context, input/output workspace and verifier bindings against authenticated stored records; matching hash-shaped strings or a worker issuer claim cannot authenticate evidence.

**G32-r22-receipt-roles:** Context preflight proves an observed launch context only; supervisor-observed execution proves process facts only; an independent verifier proves its specific check outcome. No receipt role is silently upgraded into another.

**G32-r22-artifact-recheck:** Repairing, rebasing, renaming or replacing the selected output invalidates verification for old bytes; all admitted and promoted artifacts are the exact independently checked versions.

**G32-r22-evidence-laundering:** Mutated receipts, forged issuer identities, hash-only success, missing attempts and cross-tenant references cannot cross independent admission even when individually schema-valid.
