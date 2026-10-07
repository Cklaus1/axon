---
id: CX-30
title: "Repository Improvement Plane: governed self-improvement for arbitrary projects"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-03", "CX-10", "CX-11", "CX-16", "CX-19", "CX-21", "CX-29"]
first_stage: M1
implementation_evidence: []
---

# CX-30 — Repository Improvement Plane

## 1. Purpose

Generalize Axon's reflexive self-improvement machinery from MiCode/Axon internals to any owner-approved repository or project. A repository becomes a governed optimization environment only after it declares a typed improvement contract that defines its identity, authority boundary, protected areas, accepted evidence, rollback semantics and what "better" means.

The system must not treat repository improvement as unrestricted autonomous editing. Every change remains an explicit challenger evaluated against project-local contracts and the normal Axon authority/admission path.

## 2. ProjectImprovementContract

Each managed project exposes a versioned contract:

```text
ProjectImprovementContract {
  project_id,
  repository_identity,
  source_revision,
  project_family?,
  goals[],
  non_goals[],
  protected_paths[],
  authority_profile,
  allowed_improvement_classes[],
  build_adapters[],
  test_adapters[],
  benchmark_adapters[],
  static_analysis_adapters[],
  acceptance_contracts[],
  resource_budgets,
  data_egress_policy,
  rollback_strategy,
  deployment_boundary,
  corpus_role,
  owner_approval_ref
}
```

A contract version is immutable once used by an experiment. A candidate cannot change its own project contract, protected paths, acceptance evidence or authority profile.

## 3. Repository optimization loop

For each project:

```text
observe repository + history + live episodes
→ establish baseline
→ identify friction/failure/opportunity
→ create ImprovementIntent
→ select/compose/generate challenger
→ isolated build/test/benchmark/replay
→ compare against incumbent under same contract
→ shadow/canary where meaningful
→ protected admission
→ promote or rollback
```

The loop may optimize correctness, maintainability, performance, cost, latency, test quality, build time, context use, developer ergonomics, semantic policies or other project-declared objectives. It may not invent a success criterion after seeing results.

## 4. Project-local scope and authority

Repository improvement runs under project-scoped capabilities. The optimizer receives only the authority granted by the project contract and active principal. It cannot infer permission from source text, issue descriptions, CI configuration or previous successful actions.

Protected paths and external effects remain protected even when a benchmark would improve by changing them. A project contract may require explicit human approval for selected classes such as dependency upgrades, schema migrations, deployment configuration, security policy or public API changes.

## 5. Baselines and evidence

Every project establishes a baseline bundle before challengers are compared:

- repository snapshot/revision;
- toolchain/runtime versions;
- build/test/benchmark commands through registered adapters;
- acceptance-contract version;
- resource budgets;
- known flaky/unsupported checks;
- incumbent quality/cost/latency measurements;
- environment fingerprint sufficient for replay.

A project improvement claim is always qualified by the contract and environment it was measured under.

## 6. Improvement classes

The first supported classes should include:

- semantic working-set/context policies;
- model/rule/skill routing;
- test selection and verification plans;
- build-plan/task-DAG composition;
- semantic retrieval/perception policies;
- repeated error/failure handling patterns;
- performance transformations with executable benchmarks;
- project-specific specialized Reflexes or Neural Programs;
- deterministic refactors and generated-code reductions;
- reusable local tools/procedures.

Higher-risk classes such as authorization logic, cryptographic code, destructive migrations and production deployment policy require stricter external review and may be excluded entirely by the contract.

## 7. Improvement queue

Projects maintain a typed queue of opportunities rather than allowing arbitrary continuous mutation:

```text
ProjectImprovementCandidate {
  candidate_id,
  project_contract_ref,
  improvement_intent_ref,
  target_component_or_scope,
  incumbent_revision,
  challenger_revision,
  hypothesis,
  expected_metrics[],
  required_evidence[],
  risk_class,
  rollback_target,
  status
}
```

Candidates can originate from failures, regressions, repeated expensive cognition, profiling, static analysis, human requests, cross-project patterns or Cortex discovery. Origin does not change admission requirements.

## 8. Isolation and rollback

Code challengers execute in isolated snapshots/worktrees/sandboxes. External-effect tests use mocks, disposable infrastructure or explicit staging capabilities unless the project contract grants something stronger. Rollback must be tested before any candidate canary that changes persistent state.

A code rollback is not represented as undoing irreversible external effects. OutcomeUnknown and reconciliation semantics from CX-03/CX-10 continue to apply.

## 9. Project adapters

The plane defines adapters rather than hardcoding language ecosystems:

```text
observe(project)
build(project, snapshot)
test(project, snapshot, selector?)
benchmark(project, snapshot, suite)
analyze(project, snapshot)
verify(project, snapshot, acceptance_contract)
rollback(project, activation)
```

Adapters are versioned capabilities. Their outputs become evidence artifacts with provenance, not trusted prose.

## 10. Integration with MiCode

MiCode is the primary software-world executor and data producer for repository improvement. `/build-loop`, worktrees, verification, semantic supervision, working-set management and canonical episodes should be reusable as project adapters rather than special-case Axon-only infrastructure.

CX-16 carries the contract/episode interchange. A repository can be improved through MiCode while Axon remains the evaluator, learner and promotion authority defined by the active contract.

## 11. Acceptance gates

**G30-contract:** a project cannot enter the improvement plane without an immutable ProjectImprovementContract containing repository identity, authority, protected areas, acceptance evidence, budgets and rollback semantics.

**G30-baseline:** every challenger comparison binds to a reproducible incumbent baseline and environment fingerprint; missing or materially different baselines are reported as incomparable rather than improvements.

**G30-authority:** repository challengers cannot modify protected paths, acceptance contracts, authority profiles, evaluator definitions or deployment boundaries outside explicitly granted project capabilities.

**G30-isolation:** executable challengers run in isolated/disposable environments appropriate to their effects; no production side effect is required merely to score a candidate.

**G30-evidence:** promotion requires project-declared executable evidence and cannot rely only on model/self-review, repository popularity, generated rationale or synthetic labels.

**G30-rollback:** any canary/promotion class that can change persistent project state has a tested rollback/reconciliation plan bound to a previous-good artifact.

**G30-risk-class:** the improvement class is classified before execution, and risk-class-specific approval/evidence requirements are enforced rather than inferred after results are known.

**G30-replay:** project-local improvement episodes preserve sufficient state, candidate, effective-input and adapter-version receipts to replay or explain why exact replay is impossible.

## 12. First deliverable

Use two or three owner-approved repositories with different stacks. Define contracts, baseline them, run only low-risk no-effect or isolated challengers, and produce evidence-backed proposals without automatic merging. Demonstrate that the same optimizer protocol works across projects without erasing project-specific acceptance semantics.
