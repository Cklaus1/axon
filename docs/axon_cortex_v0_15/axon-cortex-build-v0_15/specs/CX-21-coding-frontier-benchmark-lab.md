---
id: CX-21
title: "Coding Frontier / Benchmark Lab: protected whole-system capability measurement"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-10", "CX-11", "CX-12", "CX-16", "CX-19"]
first_stage: M1
implementation_evidence: []
---

# CX-21 — Coding Frontier / Benchmark Lab

## 1. Purpose

Axon Cortex needs one protected answer to a system-level question that no component is allowed to grade for itself:

> Did the complete coding system become better at solving verified software tasks under fixed authority, evidence, cost, latency and compute constraints?

The Coding Frontier / Benchmark Lab is the independent whole-system evaluation substrate for that question. It measures the combined behavior of intent resolution, observation, retrieval, Reflex, reasoning, generation, planning, world models, experiment selection, execution, verification, memory, knowledge reuse and crystallized capabilities.

CX-20 evaluates decision-model research. CX-21 evaluates the **entire coding intelligence**. A component may improve its own metric while the overall system gets worse; CX-21 exists to detect that failure mode.

The Lab is an evaluation and evidence system. It is not a production executor, training loop, authority source or self-modifying benchmark optimizer.

## 2. Decisive fork

Optimize against a protected portfolio of **verified task outcomes**, not an aggregate internal reward invented by the system being optimized.

The primary object of measurement is a task episode with:

- immutable task/intention contract;
- frozen authority and tool profile;
- resettable starting state;
- protected acceptance evidence;
- explicit resource budget;
- exact system/artifact configuration;
- complete outcome and failure accounting.

Internal metrics are diagnostic. They do not replace the protected task result.

## 3. What the Lab measures

The Lab owns benchmark definitions and reporting for at least these capability dimensions:

1. localized bug repair;
2. multi-file debugging;
3. feature implementation;
4. refactoring under behavior constraints;
5. performance optimization;
6. compiler/language implementation;
7. unfamiliar-repository comprehension;
8. cross-module and dependency reasoning;
9. concurrency/state failures;
10. security and authority-sensitive changes;
11. formal/proof obligations where supported;
12. underspecified intent clarification;
13. tool/skill invention and reuse;
14. repository-family transfer;
15. cross-language/ecosystem transfer;
16. novel task-family transfer;
17. long-horizon coding under fixed budgets.

The portfolio begins small. Categories become promotion-relevant only when their fixtures, acceptance contracts and contamination controls are reviewed.

## 4. Verified Coding Frontier

The Lab defines **Verified Coding Frontier (VCF)** as a family of curves, not a single score.

For a registered task distribution and fixed resource envelope, report the highest difficulty/novelty region at which the system maintains preregistered verified task quality.

Example axes:

```text
same repository
  → unseen commit
  → unseen repository
  → unseen repository family
  → unseen framework/ecosystem
  → unseen language
  → novel task family
```

For each region report at minimum:

```text
verified completion
coverage / abstention
human intervention
wall time
model latency
model/tool requests
input/output tokens
inference cost
build/test executions
peak host resources where available
rollback / regression rate
policy/authority refusals
```

A system does not move the frontier by spending unbounded additional compute. Frontier comparisons bind a declared resource envelope or publish a Pareto surface over quality, time and cost.

## 5. Benchmark architecture

```text
Task source / curriculum
        ↓
Task Registry
        ↓
Contamination + eligibility checks
        ↓
Frozen TaskContract / IntentIR
        ↓
Resettable environment
        ↓
System-under-test manifest
        ↓
Run controller
        ↓
Protected acceptance verifier
        ↓
Episode + evidence bundle
        ↓
Frontier aggregation
        ↓
Independent comparison / admission evidence
```

### 5.1 Task Registry

Each benchmark task has an immutable identity and records:

```text
BenchmarkTask {
    task_id
    family_id
    repository_id
    repository_family
    language_ecosystem
    source_revision
    start_snapshot
    intent_or_task_contract
    authority_profile
    acceptance_contract
    hidden_or_protected_evidence
    difficulty_metadata
    contamination_tags
    reset_recipe
    resource_envelope
    licensing_and_data_use
}
```

Task authorship may be human, imported, MiCode-derived or generated. Origin is always retained.

### 5.2 System-under-test manifest

Every benchmark run binds the complete relevant configuration:

- Cortex/Axon commit;
- active Intent compiler version;
- observer/representation version;
- retrieval index/knowledge snapshot;
- Reflex runtime + model/calibrator;
- reasoner/generator model identities;
- planner/world-model versions;
- tool/capability catalog;
- verifier version;
- memory policy/state where allowed;
- crystallized skills/tools/compiler/runtime features;
- environment/toolchain/container or host profile;
- budgets and routing thresholds.

A comparison that changes more than the preregistered experimental factor is labelled a system comparison, not a component ablation.

### 5.3 Protected verifier

The system under test may request and observe ordinary permitted evidence. It may not edit, redefine or selectively suppress the protected completion contract.

`DONE`, model confidence, critic scores, world-model predictions and internal reward do not close a task. The protected verifier does.

## 6. Benchmark tiers

### Tier A — deterministic conformance

Tiny tasks proving plumbing, authority, replay, crash handling and acceptance semantics.

### Tier B — controlled coding skills

Small/medium tasks with narrow ground truth and strong resetability. Used for iteration and ablation.

### Tier C — repository transfer

Held-out repositories/families with protected splits and no task-specific tuning.

### Tier D — capability frontier

Hard, sparse, adversarial or long-horizon tasks designed to expose current failure boundaries.

### Tier E — cross-domain transfer

Optional later tasks outside the dominant training ecosystem. A task enters this tier only with appropriate verifier quality and data-use rights.

Locked benchmark status is orthogonal to tier. Any tier may contain development tasks and protected promotion tasks.

## 7. Split and contamination discipline

The Lab treats contamination as a first-class state, not a binary guess.

Possible relationships include:

```text
ExactSeen
CommitRelated
RepositorySeen
RepositoryFamilySeen
FrameworkSeen
LanguageSeen
TaskTemplateSeen
Unknown
ProtectedUnseen
```

Promotion claims state which relationships they exclude.

Repository-level or stronger holdouts are required for cross-repository generalization claims. Cross-language claims require the relevant language/ecosystem to be excluded according to the registered split policy.

Once a protected task enters training, retrieval, curriculum generation or prompt examples, it is retired from future unseen claims for affected artifacts.

## 8. Generated curriculum and benchmark independence

CX-12 may generate tasks just beyond the current frontier. Those tasks may be excellent training material, but self-generated curricula do not automatically become protected benchmarks.

A generated task may enter the Benchmark Lab only after:

1. independent validation of the task and reset recipe;
2. an acceptance contract not editable by the candidate under evaluation;
3. leakage/solution-artifact review;
4. difficulty metadata derived without exposing the protected solution;
5. assignment to a future suite before the candidate sees its protected outcome.

The same controller must not both adapt to a task and certify that task as unseen evidence.

## 9. Longitudinal evaluation

Self-optimization creates a special risk: optimizing the visible scoreboard while losing prior abilities.

Every admitted system release therefore reports:

- current frontier;
- previous-release frontier under the same suites where still valid;
- regression matrix by capability family;
- gains/losses by cost and latency envelope;
- newly contaminated/retired tasks;
- newly added task families;
- rollback eligibility.

A gain in one family does not erase a regression in another.

## 10. Ablation and attribution

Whole-system improvement should be localized where practical.

Registered comparisons may disable or replace:

```text
Reflex
world model
retrieval
planner
memory
knowledge registry
crystallized tool
compiler optimization
representation
intent resolution policy
```

Ablations must preserve task contracts, authority and verifier. If disabling a component necessarily changes available authority or task information, that change is stated rather than disguised as a clean ablation.

The Lab consumes CX-10 failure-attribution data but does not accept attribution as proof of cause without appropriate comparison.

## 11. Metrics and reporting

No single scalar is the authoritative benchmark objective.

Required headline fields include:

- verified completion / partial completion where the contract supports it;
- task coverage and abstention;
- time-to-verified-result;
- model/inference cost;
- token and request counts;
- tool and build/test counts;
- human intervention;
- safety/authority refusals;
- regressions and rollback events;
- transfer tier;
- confidence interval or uncertainty treatment appropriate to the task sample.

Derived summaries may include Pareto dominance and frontier area, but the raw component metrics remain visible.

## 12. Benchmark gaming defenses

The Lab explicitly tests for:

- benchmark-specific file/path recognition;
- hidden-test probing;
- memorized patch lookup;
- acceptance-evidence tampering;
- refusal to attempt hard eligible tasks to inflate success rate;
- excessive compute hidden behind retries/subagents;
- use of unauthorized network/retrieval sources;
- evaluator-version exploitation;
- contamination through MiCode or external knowledge ingestion;
- task-family overfitting.

A benchmark defense may detect and invalidate evidence; it does not create new execution authority.

## 13. Relationship to other specs

- **CX-01** defines evaluation/evidence principles used here.
- **CX-10** supplies replayable episodes, lineage and failure attribution.
- **CX-11** consumes benchmark evidence for promotion/admission.
- **CX-12** may use frontier gaps to allocate curriculum/experiments but cannot edit the protected benchmark contract.
- **CX-16** supplies MiCode episodes/task candidates under provenance controls.
- **CX-17** may supply repository knowledge; benchmark contamination rules decide whether that knowledge invalidates an unseen claim.
- **CX-19** supplies typed IntentIR/acceptance semantics for intent-first tasks.
- **CX-20** evaluates Reflex research specifically; CX-21 evaluates the full coding system. A Reflex artifact may pass CX-20 and still fail to improve CX-21.

## 14. Build sequence

### Slice 1 — Registry and deterministic harness

Create task/system/run manifests, reset controller and protected verifier over a tiny conformance suite.

### Slice 2 — Baseline portfolio

Freeze initial repair/debug/feature/refactor/optimization tasks and run simple controls plus the current Cortex vertical slice.

### Slice 3 — Transfer partitions

Add repository-family and task-family holdouts with contamination metadata.

### Slice 4 — Frontier reporting

Implement fixed-envelope quality/cost/time curves and longitudinal regression reporting.

### Slice 5 — Curriculum and external-experience intake

Admit independently reviewed MiCode/generated/external tasks without mixing training and protected evaluation roles.

### Slice 6 — Admission integration

Make CX-21 evidence bundles an available/required input to CX-11 for releases claiming whole-system coding improvement.

## 15. Negative cases

The Lab must reject or downgrade evidence when:

- a protected task or solution entered training/retrieval for the candidate;
- a required hidden check is missing or candidate-editable;
- the environment cannot be reset sufficiently for the claim;
- task eligibility differs between systems without disclosure;
- failures/timeouts are dropped from the denominator;
- one system receives extra tools/authority/context outside the registered comparison;
- the evaluator changes after results are observed;
- the system claims a wider transfer level than the split supports;
- benchmark cost excludes failed retries/subagents/tool calls;
- a component metric improved while protected end-to-end task quality regressed.

## 16. Acceptance gates

**G21-contract:** a benchmark task cannot execute as promotion evidence without immutable task/intent, authority, reset, resource and protected acceptance manifests; modifying one changes the task identity or invalidates the run.

**G21-protected-verifier:** the system under test cannot edit, redefine, suppress or self-certify protected completion evidence; fake `DONE` and internal score manipulation do not produce verified completion.

**G21-contamination:** training/retrieval/knowledge exposure invalidates incompatible unseen/transfer claims, with repository/family/language/task-template relationships recorded rather than silently treated as clean.

**G21-frontier:** the report publishes verified quality/coverage plus cost/time/compute envelopes across registered novelty/difficulty regions; spending unbounded extra compute cannot masquerade as an unqualified frontier gain.

**G21-regression:** a candidate claiming whole-system improvement is checked against protected prior capability families and reports statistically/semantically meaningful regressions rather than averaging them away.

**G21-comparability:** candidate and baseline runs use matched task, authority, verifier and resource accounting or explicitly disclose every difference; failed/time-out attempts remain in accounting.

**G21-curriculum-separation:** tasks used for online adaptation/training cannot simultaneously count as protected unseen evidence for the adapted artifact; generated/MiCode tasks require independent benchmark admission.

**G21-admission-handoff:** CX-11 receives an immutable benchmark evidence bundle containing suite/system/evaluator hashes, raw outcomes, frontier summaries and claim scope; the benchmark controller cannot activate the candidate itself.

## 17. Non-goals

CX-21 does not define:

- one universal AGI/coding score;
- leaderboard marketing ranks;
- a claim that software tasks alone measure general intelligence;
- automatic deployment of benchmark winners;
- permission for the optimizer to rewrite protected tests;
- a replacement for formal proof, security review or task-specific evidence;
- a requirement that every Cortex research experiment run the full frontier suite.

## 18. Open questions

1. Which initial task families produce the best signal without excessive benchmark maintenance cost?
2. Which resource envelopes should be canonical for local, workstation and cluster evaluation?
3. How should partially verified long-horizon tasks be represented without weakening `DONE` semantics?
4. How quickly should tasks retire after public disclosure or training contamination?
5. Which transfer tiers become required for a claim of “better coder” versus a scoped capability claim?
6. How much benchmark detail can be public without making benchmark recognition itself the easiest solution?


## v0.10 completion-critic gate

**G21-stop-critic:** on a protected premature-stop suite, a completion critic can flag likely-incomplete stops and cite unresolved Intent/Acceptance clauses, but cannot create `VerifiedComplete`; hidden verifier failures override the critic and false-continue cases cannot prevent the protected verifier from closing a genuinely complete task.


## v0.13 self-hosting benchmark role
CX-21 supplies protected whole-system evidence for self-applied MiCode/Axon challengers. Production feedback may create training/development cases but must not silently change the locked coding-frontier population used for admission claims.
