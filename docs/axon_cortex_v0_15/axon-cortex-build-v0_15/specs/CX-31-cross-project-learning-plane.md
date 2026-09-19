---
id: CX-31
title: "Cross-Project Learning Plane: transfer, pattern mining and promotion across repository families"
status: Draft
authority: Proposed
depends_on: ["CX-09", "CX-10", "CX-11", "CX-17", "CX-22", "CX-30"]
first_stage: M6
implementation_evidence: []
---

# CX-31 — Cross-Project Learning Plane

## 1. Purpose

Learn reusable engineering knowledge from many governed repository-improvement episodes without assuming that a pattern observed in one project generalizes to another. The plane turns project-local evidence into scope-qualified patterns, tests transfer on held-out repository families, and promotes only evidence-backed shared capabilities.

The objective is a controlled ladder:

```text
project-local improvement
→ repeated cross-project pattern
→ scope-qualified shared capability
→ MiCode/Axon skill/tool/library
→ specialized cognition or compiler/runtime primitive when justified
```

## 2. Evidence separation

Project-local success remains local evidence. Cross-project claims require explicit project-family metadata, lineage and held-out evaluation. The system never treats many correlated repositories, forks, generated templates or duplicated examples as independent evidence.

Each episode retains:

- project/repository identity and revision;
- project family and ecosystem labels;
- source/corpus role;
- improvement contract;
- incumbent/challenger lineage;
- metrics and verifier evidence;
- license/data-use constraints;
- whether the episode was used for discovery, development, calibration or locked evaluation.

## 3. Pattern candidates

Cross-project mining may propose:

- reusable failure signatures;
- context-selection/routing policies;
- verification/test-selection strategies;
- refactoring or performance patterns;
- dependency/build patterns;
- reusable tools or procedures;
- semantic decision definitions;
- specialized Reflex/Neural Program candidates;
- libraries or framework abstractions;
- potential compiler/runtime primitives.

Every pattern includes applicability predicates and counterexamples. "Works often" is insufficient for universal promotion.

## 4. Transfer tiers

Reuse the Coding Frontier transfer discipline with repository-oriented tiers:

```text
P0 same project / new episode
P1 same repository / new revision
P2 same project family / new repository
P3 same language/ecosystem / different architecture
P4 cross-ecosystem analogous task
P5 deliberately different or adversarial transfer
```

Promotion scope must not exceed demonstrated transfer scope. A pattern that succeeds at P1 may become a project skill but not a universal Axon primitive.

## 5. Family-aware statistics and contamination control

Evaluation reports both per-episode and family/cluster-aware statistics. Related forks, benchmark templates and near-duplicate generated projects are clustered. Discovery/development repositories cannot silently enter locked transfer sets.

When external repositories contribute evidence, CX-17 data-use/licensing rules remain binding. A project becoming unavailable or disallowed triggers lineage-based quarantine/re-evaluation for dependent artifacts.

## 6. Cross-project learner

The learning loop is:

```text
collect governed project episodes
→ cluster by task/pattern/family
→ mine repeated structures and failures
→ retrieve counterexamples
→ form scoped hypothesis
→ reproduce in synthetic/resettable fixtures where possible
→ evaluate on held-out repositories/families
→ estimate transfer/maintenance/complexity economics
→ submit shared artifact to CX-11/CX-18/CX-22
```

The learner may recommend a narrower project-family capability rather than forcing global abstraction.

## 7. Promotion targets

Depending on evidence, a successful cross-project pattern may become:

```text
shared MiCode skill
shared semantic rule / retrieval policy
composition template
specialized Reflex
Neural Program
explicit tool/procedure
library/package
compiler analysis/pass
runtime primitive
```

Each target has its own admission requirements. Lower-level/native promotion requires stronger transfer and semantic-stability evidence than a project-family skill.

## 8. De-generalization

Promotion is reversible not only by version rollback but by **scope narrowing**. If later evidence shows an allegedly general pattern fails in a project family, Cortex can:

- restrict applicability predicates;
- split the artifact into family-specific variants;
- lower automatic routing confidence;
- require escalation outside validated scope;
- demote it from compiler/runtime status back to a higher cognitive layer.

Negative transfer is retained as knowledge.

## 9. Feedback to repository optimization

CX-31 supplies candidate patterns to CX-30, but a shared pattern cannot bypass the receiving project's contract. Each target repository evaluates the candidate under its own baseline, authority and acceptance criteria before local activation.

This ensures cross-project learning accelerates projects without turning shared knowledge into implicit authority.

## 10. Acceptance gates

**G31-lineage:** every cross-project pattern enumerates the project episodes, families, corpus roles, transformations and restrictions that contributed to it; untraceable aggregate evidence is not promotable.

**G31-family-split:** related repositories/forks/templates remain in the same discovery/development/evaluation partition unless an explicit contamination-safe reason is recorded.

**G31-counterexample:** pattern mining actively searches for contradictory and negative-transfer examples; discovered failures narrow scope or block general promotion.

**G31-transfer:** any shared/general claim is evaluated on held-out repository families at the transfer tier claimed by the artifact; project-local success cannot be relabeled as transfer.

**G31-scope:** an artifact's applicability/promotion scope cannot exceed the strongest transfer tier supported by protected evidence.

**G31-local-contract:** shared candidates still pass each receiving project's ProjectImprovementContract and cannot bypass project-local authority, protected paths or acceptance evidence.

**G31-degeneralize:** promoted shared artifacts support rollback or applicability narrowing when later evidence shows negative transfer, without deleting the contradictory evidence.

**G31-native-promotion:** library/compiler/runtime promotion requires stronger semantic stability, reproducibility and transfer evidence than skill-level reuse; frequency alone is insufficient.

## 11. First deliverable

Run a cross-project pilot on at least three distinct repository families using previously admitted project-local episodes. Discover one reusable pattern, one pattern that must remain family-specific, and one negative-transfer case. Demonstrate that evidence scope determines promotion scope.
