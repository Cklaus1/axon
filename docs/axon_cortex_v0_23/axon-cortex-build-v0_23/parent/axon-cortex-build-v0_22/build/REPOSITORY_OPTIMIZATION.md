# Repository Optimization Plane — implementation guide

This guide operationalizes CX-30 and CX-31. The goal is to make the existing Cortex self-application machinery usable across arbitrary repositories without flattening project-specific semantics.

## Architecture

```text
ProjectImprovementContract
        ↓
Repository observer / MiCode adapters
        ↓
project baseline + improvement queue
        ↓
ImprovementIntent
        ↓
challenger builder
        ↓
isolated replay/build/test/benchmark
        ↓
project-local verifier
        ↓
CX-11 admission / bounded activation
        ↓
project episode store
        ↓
CX-31 cross-project learner
        ↓
shared candidate patterns
        ↓
back through each target project's contract
```

## Rules

1. A repository is not an optimization target until its contract is versioned and owner-approved.
2. The optimizer cannot rewrite the contract used to judge itself.
3. Build/test/benchmark commands are registered adapters, not arbitrary generated shell strings.
4. Project-local evidence stays project-local until transfer is measured.
5. Cross-project patterns always retain family/lineage metadata and counterexamples.
6. Shared capabilities never bypass receiving-project authority.
7. Promotion scope is bounded by transfer evidence.
8. Negative transfer is a first-class result and may trigger de-generalization.

## Minimum ProjectImprovementContract

The first schema should require identity/revision, protected paths, capability profile, accepted improvement classes, build/test/benchmark adapters, acceptance contracts, budgets, data-egress policy and rollback strategy. Optional fields may describe project family, deployment staging and domain-specific metrics.

## Initial project-local targets

Prefer reversible/high-observability optimizations:

- context and working-set selection;
- test selection;
- model/rule/skill routing;
- build-plan composition;
- semantic retrieval definitions;
- repeated deterministic refactors;
- benchmark-backed performance changes in isolated environments.

Do not begin with production deployment, auth/security policy, destructive migrations or irreversible external effects.

## Cross-project promotion ladder

```text
project-specific rule/skill
→ project-family shared skill
→ ecosystem-level procedure/library
→ specialized cross-project model
→ compiler/runtime primitive
```

Each transition needs new evidence rather than inheriting confidence from the prior scope.

## MiCode role

MiCode should implement the repository adapter/execution side first: worktree/snapshot creation, semantic observation, `/build-loop`, supervisor, protected verifier, replay records and project evidence export. Axon owns the generic contracts, learning, transfer evaluation and promotion policy.

## Required artifacts

- `ProjectImprovementContract`
- `ProjectBaseline`
- `ProjectImprovementCandidate`
- `RepositoryExperimentReceipt`
- `CrossProjectPatternCandidate`
- `TransferEvaluationReceipt`
- `SharedCapabilityProposal`
- `ApplicabilityRevision`

## First pilot

Choose 2–3 repositories with different build systems. Run a no-merge pilot that proves one common protocol can create baselines, execute isolated challengers and emit comparable evidence while preserving project-specific acceptance criteria. Only after that run the first cross-project pattern-mining experiment.
