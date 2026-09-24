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

## v0.16 review amendment — Dispatch, fallback and current revocation

The canonical registry may store non-active candidates, but production discovery sees only scope-authorized admitted revisions. At dispatch, atomically bind active release, target, input projection, policy generation and resource reservation. Recheck current revocation/expiry even on cache hits. Fallback is a bounded graph of new invocations with inherited budgets and hard rechecks; no model-generated edge may escape locality/authority. Fair scheduling and separate shadow/learning quotas protect foreground control paths.

**G34-revocation-race:** revoke or change a release/tenant/project binding between selection and dispatch; stale aliases/cache leases cannot authorize new protected work, and an invalid previous-good release is not activated.

**G34-bounded-fallback:** fallback cycles, budget resets, shadow starvation and local-denial-to-remote-routing are blocked under the registered policy; unsupported state operations remain unsupported.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE physical profile and dispatch

P02/P03/P08 attach physical profiles/features to current registry/selection records, not a parallel plan authority. Hard filtering and current eligibility recheck precede execution. Explicit bounded fallbacks preserve remaining constraints/budgets and cannot bypass auth, artifacts or offline restrictions.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G34-ace-dispatch:** A chosen physical profile binds capability/contract/release/features/input/policy/reservation; revoke or change any binding before dispatch and the stale choice refuses, including on cache hits.

**G34-ace-fallback:** Fallback graph cycles, budget resets, hidden producers and permission/offline bypasses fail; every fallback is a separately identified constrained invocation with truthful operator state.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

Exact-compatible lightweight heads may reuse immutable encoder assets or authorized features only under the [qualified identity and isolation profile](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md). Sharing model files is not permission to share private embeddings, mutable adapters or model-native state. Bind tenant/project/principal/partition and runtime/precision/projection in caches; bound lifetime/reference counts/memory and cancellation. Current revocation is checked at dispatch; pin in-flight generation and reconcile unknown effects before any retry. A stale deployment or null cutoff cannot be silently repaired with an unqualified head.

**G34-shared-encoder-isolation:** Exact-compatible shared encoder assets/features preserve privacy/evaluator scope, runtime identity, bounded lifetimes and memory; disposing/canceling one head cannot corrupt another or authorize cross-principal feature reuse.

**G34-reflex-revocation:** Drift/revoked-data/release changes stop new uses at dispatch, preserve in-flight identity and evidence, and select only a qualified authorized previous-good/incumbent fallback after unknown-effect reconciliation.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G34-r21-candidate-view:** Candidate views are derived from the existing CX-34 registry with unique semantic IDs and revision-bound descriptions; duplicated or changed candidate text cannot silently reuse old embeddings.

**G34-r21-filter-before-score:** Authority, privacy, host compatibility and budget eligibility filter candidate access before external scoring; no score, rank or provider response expands capabilities or discloses excluded candidates.

**G34-r21-route-session:** Route selection records session, model/provider revisions, state compatibility, queue and cache-switch costs; private state and incompatible caches are not silently transferred to another backend.

**G34-r21-route-control:** A deterministic eligible incumbent exists independently of learned routers; unhealthy, revoked, unsupported or unaffordable routes cannot be selected by a high learned score.

**G34-r21-research-revocation:** Data/model/head/policy revocation stops new dispatch and cache eligibility; in-flight work remains bound to its recorded epoch and cannot be relabeled as current or admitted after revocation.

**G34-r21-research-fallback:** Fallback is qualified, eligible, principal-preserving and budget-bounded; errors never retry under a weaker scope or mix evidence across candidate sets and model epochs.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G34-r22-pilot-controls:** The pilot fixes model/role/provider version, eligible profile, verifier, authority, budgets and context settings; unknown or implicit default changes invalidate comparability rather than being attributed to the shortlist.
