---
id: CX-34
title: "Cognitive Scheduler and Shared Capability Registry: route work across heterogeneous intelligence units"
status: Draft
authority: Proposed
depends_on: ["CX-03", "CX-04", "CX-06", "CX-11", "CX-13", "CX-18", "CX-22", "CX-23", "CX-24", "CX-26", "CX-28", "CX-31", "CX-32"]
first_stage: M2
implementation_evidence: []
---

# CX-34 — Cognitive Scheduler and Shared Capability Registry

## 1. Purpose

Turn the expanding set of rules, semantic matchers, Reflex backends, specialized models, Neural Programs, tools, generators, world models and human escalation paths into an OS-like schedulable capability fabric rather than scattered harness heuristics.

The scheduler answers:

```text
Given this typed cognitive operation,
which authorized capability should execute it now,
under this quality / latency / cost / privacy / risk / hardware state?
```

## 2. Shared Capability Registry

Every reusable cognitive capability is registered with immutable identity and versioned metadata:

```text
CapabilityArtifact {
  id/revision
  semantic_contract
  input/output types
  applicability predicates
  authority/effect ceiling
  evidence + transfer tier
  known failures / OOD boundaries
  quality/calibration profile
  latency/cost/energy profile
  locality/privacy constraints
  hardware/runtime requirements
  cache/residency hints
  fallback graph
  rollback/supersession lineage
}
```

Candidate discovery happens only over already authorized/compatible registry entries. Learned models cannot mint executable capabilities during routing.

## 3. Cognitive scheduler

The scheduler chooses among strategy/capability classes such as:

```text
RULE
RETRIEVE
SEMANTIC_MATCH
SELECT/PROJECT
COMPOSE
SPECIALIZED_REFLEX
GENERAL_REFLEX
NEURAL_PROGRAM
TOOL/PROCEDURE
GENERATE
THINK
SIMULATE
PROVE
HUMAN
```

Selection combines hard constraints with a utility model. Hard constraints include authority, type compatibility, privacy/data-egress, required evidence, hardware availability and risk class. Soft objectives may include predicted quality, latency, monetary cost, energy, cache warmth, transfer confidence and opportunity cost.

## 4. Decision-theoretic utility

Confidence alone is insufficient. Scheduler policy considers consequence and reversibility, for example:

```text
expected utility = task value
                 × success probability
                 − latency/cost/energy
                 − failure consequence
                 − uncertainty/OOD penalty
                 + information/reuse value
```

The exact function is versioned and domain-specific. High-impact irreversible actions may route to stronger verification/human paths even when a cheap capability is highly confident.

## 5. Hardware-aware cognition

Runtime resource state may affect scheduling:

- CPU/GPU/NPU availability;
- memory pressure;
- accelerator/model residency;
- KV/semantic cache warmth;
- network/offline state;
- power/energy budget;
- local-vs-remote privacy constraints;
- queue depth and concurrency.

Hardware state changes performance policy, never semantic authority.

## 6. Cognitive cache integration

Capabilities may expose reusable caches for:

- decision results;
- semantic retrieval;
- world-state projections;
- proof/verification artifacts;
- compiled skills/models;
- shared prefix/KV state.

Cache keys bind to semantic state and artifact revisions. Stale or cross-tenant reuse is rejected.

## 7. Cascades and abstention

The scheduler supports explicit fallback graphs with `NONE`, `UNKNOWN`, `OBSERVE_MORE`, `ESCALATE` and `BLOCKED` outcomes. A failed/abstaining cheaper capability may route upward without pretending it produced a valid answer. Every escalation is recorded in the cognitive cascade/evidence graph.

## 8. Capability lifecycle

The registry accepts artifacts only through existing admission paths. CX-22 can propose specialization, CX-31 can propose shared capabilities, and CX-18 can propose native promotion, but registry publication never bypasses CX-11 evidence/admission. Superseded or drifted capabilities can be quarantined, scope-narrowed or rolled back.

## 9. Integration

- CX-06 provides calibration/risk-aware routing signals.
- CX-13 exposes runtime resource/budget state.
- CX-22/CX-23 supply specialized and neural-program capabilities.
- CX-24/CX-26 supply semantic perception/retrieval and composition capabilities.
- CX-28 provides working-set/cache context.
- CX-31 contributes transfer-scoped shared capabilities.
- CX-32 links registry artifacts and scheduler decisions to evidence.

## 10. Acceptance gates

**G34-registry-identity:** every schedulable capability has immutable revision, semantic contract, type/effect ceiling, applicability and evidence lineage; mutable names are aliases only.

**G34-authorized-candidates:** scheduler candidate enumeration excludes unauthorized, incompatible, revoked, stale or privacy-forbidden capabilities before learned ranking and rechecks them at dispatch.

**G34-hard-before-soft:** hard authority/type/privacy/evidence constraints are applied before utility scoring; no cost/quality advantage can override them.

**G34-utility-lineage:** scheduler decisions record policy revision, considered candidates, hard exclusions, predicted utility components, chosen fallback and realized cost/outcome.

**G34-risk-reversibility:** routing incorporates action consequence/reversibility class; high-confidence cheap models cannot silently lower approval/verification requirements for high-impact operations.

**G34-hardware-aware:** hardware/cache/load state may change performance routing but cannot alter semantic authority or reuse artifacts across incompatible state/tenant boundaries.

**G34-abstention:** NONE/UNKNOWN/OBSERVE_MORE/ESCALATE/BLOCKED are preserved as distinct outcomes and trigger explicit fallback/observation paths rather than forced choices.

**G34-cache-freshness:** decision/retrieval/proof/model caches bind to semantic state and artifact versions; stale or cross-scope reuse is rejected and auditable.

**G34-admission-only:** new/shared/specialized capabilities enter the active registry only after their owning admission path; scheduler learning cannot self-publish a candidate.

## 11. First deliverable

Register a deterministic rule, semantic matcher, specialized/general Reflex, generator and human escalation mock behind one typed operation. Run a scheduler benchmark under varying latency/cost/hardware/privacy constraints and demonstrate correct hard-constraint filtering, abstention/fallback, cache invalidation and rollback to a previous-good capability revision.
