---
id: CX-26
title: "Decision Composition Runtime: select, project and compose typed world artifacts before generating new content"
status: Draft
authority: Proposed
depends_on: ["CX-02", "CX-04", "CX-05", "CX-11", "CX-19", "CX-24"]
first_stage: M2
implementation_evidence: []
---

# CX-26 — Decision Composition Runtime

## 1. Purpose

Cortex should not ask a generative model to recreate information or structures that already exist as typed, inspectable, authorized artifacts. The Decision Composition Runtime (DCR) introduces a general **select / project / copy / compose** path between retrieval and generation.

The runtime answers a simple question before invoking open-ended synthesis:

> Can the requested result be obtained by selecting, projecting, copying, ordering or composing existing typed artifacts under the approved Intent IR and capability policy?

If yes, Cortex should prefer bounded selection/composition and deterministic validation. GENERATE remains the fallback when the required artifact does not exist or composition cannot satisfy the contract.

This generalizes the same pattern across browser control, structured extraction, UI/spec composition, build planning, verification-plan construction, compiler-pipeline assembly and repository operations.

## 2. Decisive fork

Treat composition as a first-class cognitive execution strategy, not as an implementation detail inside GENERATE.

The preferred routing order is conceptually:

```text
RULE
  ↓
RETRIEVE
  ↓
SELECT / PROJECT / COPY
  ↓
COMPOSE
  ↓
REFLEX
  ↓
SPECIALIZED COGNITION
  ↓
GENERATE
  ↓
THINK / SIMULATE / EXPERIMENT / PROVE
```

This ordering is a policy heuristic, not a claim that every task must follow every stage. Authority-sensitive predicates and exact compiler facts remain deterministic even if a learned model could imitate them.

## 3. Core objects

The runtime operates on typed catalog entries rather than model-authored executable strings.

```text
ArtifactCandidate {
  candidate_id,
  semantic_type,
  source_object_ref,
  snapshot_digest,
  provenance_refs[],
  value_or_projection_ref?,
  authority_requirements[],
  applicability_predicate?,
  freshness_predicate?,
  metadata
}

ArtifactCatalog {
  catalog_id,
  intent_ref,
  state_ref,
  candidate_policy_ref,
  candidates[],
  omitted_candidates[],
  order_policy,
  catalog_digest
}
```

Candidate generation is outside the model's authority. A model may rank or select only artifacts that the catalog compiler exposes.

## 4. Operations

### 4.1 SELECT

Select one or more existing candidates from a bounded catalog.

Examples:

- choose the observed symbol that satisfies a semantic request;
- choose the test or verifier to run;
- choose the DOM element whose text should be copied;
- choose the compiler pass or task template to include.

### 4.2 PROJECT / COPY

Return an existing authoritative value or a deterministic projection of it rather than regenerate it.

Examples:

- copy a type from the compiler's authoritative type map;
- copy an installed version from a lockfile object;
- project a URL, number, label or source span from a selected object;
- copy an already-produced artifact into a new composition.

Projection rules are versioned deterministic code. Learned output is never treated as the authoritative value when the value already exists in a trusted source.

### 4.3 COMPOSE

Assemble a valid artifact from known pieces and registered relationships.

Possible composition targets include:

- build-task DAGs;
- verification plans;
- UI/spec trees;
- tool chains;
- compiler-pass pipelines;
- action plans;
- library/skill assemblies;
- partial program graphs.

Composition is constrained by a schema/catalog and validated before execution or admission.

## 5. Dependency semantics

A composition graph carries explicit dependency classes from CX-15/CX-05:

- `Independent` candidates can be decided in the same batch;
- `ConditionallyRelevant` candidates may be speculatively evaluated under a branch assumption and discarded if the branch is inactive;
- `AnswerDependent` nodes require the prior answer or an explicit bounded expansion of all possible branches.

A composition engine must not convert answer-dependent semantics into a single joint prompt merely for latency.

## 6. Partial composition and fallback

The runtime distinguishes:

- `CompleteComposition` — all required fields/edges are resolved and valid;
- `PartialComposition` — a valid prefix/subgraph exists but one or more required slots remain unresolved;
- `NoSuitableCandidate` — current catalog has no acceptable artifact;
- `NeedMoreObservation` — additional world state could reveal a suitable artifact;
- `BlockedByAuthority` — a suitable artifact exists but is outside approved authority;
- `InvalidComposition` — selected artifacts violate schema/invariants;
- `GenerateFallback` — bounded composition cannot satisfy the contract and open-ended synthesis is permitted.

These states must not collapse into one generic model failure.

## 7. Composition verifier

The DCR never self-certifies its artifact. Validation may include:

- type/schema validation;
- graph/DAG validity;
- uniqueness/cardinality constraints;
- authority/effect checks;
- snapshot/freshness checks;
- semantic invariants;
- hidden/protected acceptance checks when appropriate.

A `finish` or `complete` model choice is only a proposal. Protected verification remains authoritative.

## 8. Integration with Intent IR

Intent clauses are traceable into the composed artifact.

```text
CompositionTrace {
  intent_digest,
  catalog_digest,
  selected_candidate_ids[],
  projection_rule_ids[],
  composition_graph_digest,
  clause_to_artifact_nodes,
  unresolved_clause_refs[],
  fallback_generation_refs[]
}
```

The composer may narrow an implementation choice or add stronger evidence steps. It cannot silently drop a `MUST`, widen authority, weaken acceptance evidence or convert a preference/hypothesis into a hard requirement.

For build work, the preferred path is:

```text
Solution description
→ Intent IR
→ typed task/evidence catalog
→ select + compose known tasks/templates
→ Build Spec / Task DAG
→ GENERATE only genuinely novel tasks
→ /build-loop or equivalent executor
```

## 9. Learning and self-optimization

Composition is itself a crystallization target.

If THINK/GENERATE repeatedly creates the same structure, Cortex may propose:

1. a reusable artifact catalog;
2. a deterministic projection rule;
3. a composition policy;
4. a skill/tool/template;
5. later, a compiler/runtime primitive under CX-18.

The system should compare composition against generation on verified utility, latency, cost, robustness, transfer and authority surface.

## 10. Relationship to other specs

- CX-02 supplies structured world objects and provenance.
- CX-04 schedules composition nodes in AIR.
- CX-05 supplies bounded selection decisions and distributions.
- CX-19 supplies approved Intent IR and clause traceability.
- CX-24 supplies semantic matching/extraction used to build or rank catalogs.
- CX-20 evaluates decision/composition policy variants.
- CX-21 measures end-to-end whole-system effects.
- CX-22 may specialize recurring composition policies.
- CX-11 owns admission of reusable composition artifacts.

## 11. Acceptance gates

**G26-catalog-authority:** a model cannot add an executable candidate absent from the runtime-generated catalog; injected path/command/tool strings remain data and cannot become authority.

**G26-select-copy:** when an authoritative value exists in the catalog, SELECT/PROJECT returns the source-bound value and provenance rather than a model-regenerated substitute; stale source identity is refused.

**G26-compose-schema:** invalid ordering, cardinality, dependency or type combinations are rejected before execution; a syntactically valid but semantically incomplete `finish` cannot pass protected completion.

**G26-partial-fallback:** missing candidates produce `NoSuitableCandidate`, `NeedMoreObservation`, `BlockedByAuthority`, or an explicitly permitted generation fallback rather than a forced ordinary candidate.

**G26-intent-trace:** every consequential node in an admitted composition traces to an approved Intent IR clause or an explicitly stronger safety/evidence step; orphan nodes fail.

**G26-composition-benefit:** on a preregistered task family where the answer/artifacts already exist, select/compose is compared with generation under matched authority and verification; claimed adoption requires the registered quality floor plus measured cost/latency/robustness benefit.

**G26-dependency-semantics:** independent/conditional/answer-dependent decisions are scheduled according to declared semantics; optimization may not fuse answer-dependent branches into a behavior-changing prompt while claiming equivalence.

## 12. First build slice

Choose one coding-world task where the result already exists in the repository/compiler state, such as selecting a test/symbol/value and projecting it into a typed result. Then compose a small verification plan from registered checks. Compare against a generative baseline, inject stale/absent/unauthorized candidates, and preserve exact intent/provenance lineage. Do not begin with arbitrary code synthesis.
