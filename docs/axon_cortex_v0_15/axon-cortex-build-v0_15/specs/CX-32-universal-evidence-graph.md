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
