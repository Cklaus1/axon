---
id: CX-22
title: "Cognitive Specialization Compiler: compile recurring cognition into cheaper guarded representations"
status: Draft
authority: Proposed
depends_on: ["CX-10", "CX-11", "CX-14", "CX-20", "CX-21"]
first_stage: M5
implementation_evidence: []
---

# CX-22 — Cognitive Specialization Compiler

## 1. Purpose

Cortex should not assume that one Reflex model is the permanent execution substrate for every recurring judgment. Repeated expensive cognition may be better represented as a deterministic rule, template, specialized decision model, neural program, explicit tool/procedure, general Reflex call, or continued THINK fallback.

The Cognitive Specialization Compiler (CSC) turns verified recurring cognition into **candidate cheaper representations**, compares them under the same task/authority/evidence contracts, and promotes only when independent admission says the specialization is worthwhile and safe within a declared applicability envelope.

The objective is not "replace reasoning with classifiers." The objective is:

> For each recurring cognitive operation, find the cheapest reliable representation that preserves verified utility inside an explicit domain and falls back safely outside it.

## 2. Decisive fork

Specialize **per decision/function family**, not by choosing one global System-One architecture.

The compiler considers multiple targets:

```text
THINK / GENERATE
      ↓
GENERAL_REFLEX
      ↓
┌───────────────────────────────┐
│ SPECIALIZED_REFLEX            │
│ NEURAL_PROGRAM                │
│ TOOL / PROCEDURE              │
│ TEMPLATE                      │
└───────────────────────────────┘
      ↓
RULE / LIBRARY / COMPILER / RUNTIME
```

This is a search over representations, not a mandatory ladder. Some tasks remain general Reflex or THINK indefinitely. A learned specialization never gains authority merely because it is faster or more accurate.

## 3. Inputs

A specialization candidate starts from a versioned `CognitiveFunctionProfile`:

```text
CognitiveFunctionProfile {
  function_id,
  semantic_contract_ref,
  input_schema_ref,
  output_schema_ref,
  authority_ceiling,
  candidate_policy_ref?,
  observed_calls,
  verified_outcome_refs[],
  error/abstention/OOD history,
  latency_cost_memory_profile,
  current_executor,
  data_eligibility_manifest
}
```

The profile may describe a bounded choice, binary/ordinal decision, fuzzy transformation, extraction/normalization function, planner heuristic, verification selector, retrieval policy, or other typed operation. Exact safety predicates and authority decisions are not eligible for learned substitution when deterministic evaluation exists.

## 4. Specialization eligibility

Before training or compiling a candidate, the CSC measures:

- semantic contract stability;
- input/output schema stability;
- candidate-set/cardinality behavior if applicable;
- number and diversity of eligible verified examples;
- repository/task/language transfer coverage;
- label/evidence quality and delayed/censored outcomes;
- base-rate and class/candidate skew;
- current executor quality/cost/latency;
- OOD and abstention behavior;
- whether a deterministic implementation already exists or is cheaper to build;
- whether the operation is reversible/low-risk enough for shadow/canary research.

No fixed example-count threshold grants eligibility. Learning curves and transfer evidence decide whether more data or a different representation is justified.

## 5. Candidate representation families

At minimum the Lab can compare:

1. **Rule/template.** Deterministic implementation with explicit domain.
2. **Specialized encoder/decision model.** GLiNER/Laya-like or other schema-conditioned discriminative model where appropriate.
3. **Candidate-token scorer.** Nimble-style bounded token-logit scorer for small stable option spaces.
4. **Pointer/listwise decision model.** Kev-style or other dynamic-option model for runtime-generated candidate sets.
5. **Neural program.** CX-23 learned function/adapter that may perform typed fuzzy transformation rather than only a bounded decision.
6. **General Reflex.** Existing CX-05 backend.
7. **THINK/GENERATE.** Expensive fallback/teacher for novelty or open-ended synthesis.

Names above are research families, not adopted dependencies. Every external implementation still passes dependency/adoption review.

## 6. Specialization objective

Do not collapse quality, latency and cost into one unreviewed scalar. The experiment preregisters a quality floor and resource objectives. A useful report includes:

```text
quality / selective risk / verified task utility
latency distribution
inference cost
memory / resident artifact cost
training/compile cost
maintenance/update cost
coverage and abstention
OOD/transfer degradation
fallback rate
human intervention
```

A convenience `SpecializationROI` may summarize these only with registered weights/constraints. The underlying metrics remain authoritative.

## 7. Applicability and routing

Every specialization exports an `ApplicabilityGuard` independent of its own confidence score where possible. Inputs include model/artifact version, domain/language, schema version, candidate cardinality, repository/task family, state projection version, and known OOD signals.

The runtime routes:

```text
exact deterministic predicate → RULE
eligible specialized domain → SPECIALIZED_REFLEX / NEURAL_PROGRAM
unsupported or drifted domain → GENERAL_REFLEX
novel/high-risk/insufficient evidence → THINK / PROVE / BLOCK
```

A specialization may abstain. It cannot widen authority, alter the candidate compiler, or approve its own use outside the registered domain.

## 8. Drift, de-specialization and lifecycle

Specialized artifacts are immutable. New data produces new candidates. The system monitors realized outcomes, transfer decay, calibration drift, schema/candidate changes, representation/version changes and cost regressions.

On applicability mismatch or regression:

1. stop new uses of the specialization;
2. preserve the failed evidence and affected episode lineage;
3. route eligible future work to the previous-good/general executor;
4. decide whether to recalibrate, retrain, narrow scope or retire;
5. never rewrite historical outcomes to make the specialization look valid.

## 9. Relationship to other specs

- CX-10 provides eligible trace/data lineage.
- CX-20 runs specialization experiments and locked transfer evaluation.
- CX-21 measures whole-system benefit rather than only component accuracy.
- CX-11 owns promotion state and independent admission.
- CX-23 provides the neural-program target.
- CX-18 governs later skill/tool/library/compiler/runtime crystallization.

## 10. Acceptance gates

**G22-discovery:** from a mixed corpus of recurring/non-recurring decisions, identify a specialization family using preregistered stability/data criteria without using locked-test performance to decide eligibility.

**G22-comparison:** compare at least a general Reflex/current incumbent and one specialization candidate on the same eligible examples, authority contract and evaluator; failures and abstentions remain visible.

**G22-applicability:** inject an unseen language/schema/candidate regime outside the specialization envelope; the guard refuses or falls back instead of trusting a high softmax score.

**G22-roi:** a candidate cannot promote merely for latency/cost gain when preregistered verified-quality/coverage constraints fail; an inconclusive comparison remains INCONCLUSIVE.

**G22-drift:** after an input/schema/outcome distribution change invalidates registered evidence, the specialization is suspended or narrowed and stale calibration is not reused.

**G22-fallback:** removal/corruption/unavailability of a specialized artifact routes through the declared fallback or refuses; it never silently substitutes an unadapted/base model while claiming specialized semantics.

## 11. First build slice

Choose one low-risk recurring coding decision with verified labels (for example failure-family attribution or verification-strategy selection). Build a dataset, train/evaluate one small specialized candidate, compare against general Reflex/current behavior, enforce applicability/fallback, and submit only an evidence bundle to CX-11. Do not begin with permission/risk authorization or irreversible actions.


## v0.10 semantic specialization targets

Recurring perception/retrieval work may specialize into a `SemanticExtractor` or `SemanticMatcher` under CX-24 rather than being forced into a decision classifier. CX-22 compares these targets under the same applicability, transfer, resource and fallback discipline as other specialized cognition.

## v0.11 composition specialization target

Recurring cognition may specialize into a `CompositionPolicy` governed by CX-26. This target is appropriate when repeated THINK/GENERATE behavior mostly selects, orders or connects already-known typed artifacts rather than synthesizing novel content. A composition policy is compared against generation and general Reflex under the same applicability, verification, cost and fallback requirements as other specialization targets.

## v0.12 additional specialization targets

Recurring cognition may also crystallize into a `SupervisorPolicy`, `WorkingSetPolicy`, `ModelRoutePolicy`, `SemanticQueryPlan` or `CognitiveCascadePolicy` when protected replay shows stable benefit. Such artifacts remain policies over already-authorized operations; specialization cannot promote authority or completion semantics.


## v0.13 internal specialization targets
CX-22 may mine CX-29 records from routing, context selection, supervision, retrieval and composition. An internal component is not privileged: if a recurring decision has stable verified labels, it may be specialized or crystallized subject to the same applicability/fallback/admission requirements.

## v0.14 cross-project specialization scope

The specialization compiler may consume CX-31 pattern candidates, but any resulting specialized model/program/policy inherits the strongest demonstrated applicability scope. Project-family specialization is preferred over a falsely universal artifact when held-out transfer is mixed. Deployment into a receiving repository still passes CX-30's active project contract.
