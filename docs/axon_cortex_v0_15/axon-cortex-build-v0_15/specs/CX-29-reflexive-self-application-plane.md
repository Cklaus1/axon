---
id: CX-29
title: "Reflexive Self-Application Plane: self-observation, challenger generation and guarded self-improvement"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-10", "CX-11", "CX-12", "CX-20", "CX-21", "CX-22", "CX-25", "CX-27", "CX-28"]
first_stage: M1
implementation_evidence: []
---

# CX-29 — Reflexive Self-Application Plane

## 1. Purpose

Axon should apply its own cognitive architecture to itself. Every non-kernel cognitive subsystem must be observable, versioned, replaceable and eligible for the same replay, challenger, shadow, admission, rollback and crystallization machinery that Cortex applies to application workloads.

The first objective is **self-observation**, not autonomous self-modification. Axon begins collecting complete outcome-linked records immediately, runs challengers offline and in shadow, and only later permits bounded automatic promotion for low-risk policy classes after independent evidence.

## 2. Self-application invariant

The architecture adopts the invariant:

> Every non-kernel cognitive subsystem is a governed optimization target.

Eligible components include:

```text
Observer policy
Retriever / semantic query planner
Working-set manager
Rule / skill selector
Model router
Reflex backend and question definition
Decision composer
Planner
Supervisor
Completion critic
Neural skill
Specialization policy
```

A component may remain hand-written, learned, composed or hybrid. The requirement is that its behavior can be reconstructed from versioned inputs and compared against challengers.

## 3. Cognitive operation record

Every eligible invocation produces or links to a canonical record:

```text
CognitiveOperationRecord {
  operation_id,
  component_kind,
  component_revision,
  intent_ref,
  state_handle,
  candidate_catalog_digest?,
  effective_input_receipt,
  selected_strategy,
  output_digest,
  uncertainty_or_abstention?,
  cost,
  latency,
  authority_context,
  downstream_episode_ref,
  verifier_evidence_refs[],
  outcome_label?,
  failure_attribution?,
  privacy_and_corpus_role
}
```

The record must distinguish what the component actually saw from what the wider system possessed. Missing outcome labels remain missing; they are not back-filled by self-report.

## 4. Protected kernel boundary

Self-improvement does not apply uniformly to the system. The following remain protected roots of trust unless changed through an explicit external governance process:

- capability/effect enforcement and narrowing-only authority;
- interpreter/reference semantics and equivalence oracle;
- artifact identity/hash validation;
- admission policy and locked-evaluation ownership;
- provenance/audit durability;
- rollback and previous-good artifact recovery;
- corpus-role separation and contamination controls;
- verifier authority over protected completion.

A candidate cognitive component cannot modify the evaluator, acceptance threshold, locked suite, authority ceiling or evidence definition used to admit itself.

## 5. Universal optimization loop

Every optimization target follows the same lifecycle:

```text
OBSERVE
→ MEASURE
→ DETECT recurring pattern / inefficiency / failure
→ FORM ImprovementIntent
→ GENERATE or COMPOSE challenger
→ REPLAY on frozen episodes
→ SHADOW on live traffic
→ LOCKED EVAL / transfer checks
→ ADMISSION
→ CANARY / bounded activation
→ MONITOR
→ PROMOTE or ROLLBACK
```

The loop produces explicit lineage between incumbent, challenger, experiment, evidence and activation event.

## 6. Immediate rollout stages

### Stage A — instrument now

From the first implementation slice onward, MiCode and Axon should emit outcome-linked cognitive operation records for routing, context selection, rule/skill selection, semantic retrieval, question/criteria versions, composition, supervision and completion judgments.

### Stage B — offline replay

Historical episodes are used to compare alternative policies without changing live behavior. Replay must bind to the same world/state/candidate snapshots where comparison semantics require it.

### Stage C — shadow challengers

Challengers execute against live inputs but their outputs cannot affect tools, authority, completion or persistent state. Shadow results are joined later to actual outcomes.

### Stage D — bounded auto-promotion

Only low-risk, easily reversible policy classes may eventually receive automatic promotion authority, and only after the protected admission policy explicitly permits that class. Initial candidates include model routing, context selection, rule/skill selection, semantic-retrieval thresholds and compaction/working-set policies.

### Stage E — higher-impact self-improvement

Supervisor policy, planner/composition policy, specialized Reflexes and neural skills remain independently admitted until sufficient evidence exists for narrower automatic promotion envelopes.

## 7. Self-hosting requirement

MiCode is the first proving ground: it should use semantic working-set selection, supervisor judgments, model routing, decision composition, context GC and specialization internally while producing records about those same mechanisms.

Axon then self-hosts the same pattern. For example, the Working-Set Manager's own selection policy can become a challenger target; the Supervisor can be evaluated by an independent benchmark; the Specialization Compiler can propose cheaper representations for recurring specialization decisions.

No component is exempt merely because it participates in optimization.

## 8. Recursive optimization without evaluator capture

A subsystem may propose a successor to itself, but cannot be the sole source of truth for its own improvement claim. At minimum:

```text
active component
→ candidate successor
→ independent replay / shadow runner
→ protected evaluator
→ protected admission
```

If a supervisor proposes a new supervisor, the acceptance decision is made outside both active and candidate supervisor implementations. If a Working-Set Manager proposes context policy changes, protected pins and evaluation receipts are supplied by the kernel/runtime rather than by the candidate.

## 9. ImprovementIntent

Every proposed self-change is represented as an explicit typed improvement intent:

```text
ImprovementIntent {
  target_component,
  incumbent_revision,
  observed_problem,
  supporting_episode_refs[],
  hypothesis,
  candidate_representation,
  expected_benefit,
  protected_invariants[],
  required_suites[],
  rollback_target,
  maximum_authority_change = NONE
}
```

Discoveries do not directly mutate production. They enter the experiment/admission pipeline.

## 10. Downward crystallization

Self-application uses the same cognition ladder as application behavior. A recurring subsystem decision may descend:

```text
THINK / GENERATE
→ RETRIEVE / SELECT / COMPOSE
→ GENERAL REFLEX
→ SPECIALIZED REFLEX / NEURAL PROGRAM
→ TOOL / PROCEDURE
→ RULE / COMPILER / RUNTIME
```

The optimization objective is not merely lower latency. Promotion requires preserved or improved verified utility, transfer behavior, safety and rollback economics.

## 11. New-primitive discovery

The current AIR vocabulary is not the final cognitive ontology. CX-09/CX-12 may identify a repeated strategy that is poorly represented by existing primitives. A proposed new primitive must include:

- a typed semantic contract;
- operational semantics and authority behavior;
- replay representation;
- evidence that it improves composability, quality or cost across more than one narrow episode family;
- a lowering path or interpreter implementation;
- independent admission and rollback.

Thus the system can evolve its own cognitive vocabulary without allowing a learned component to rewrite language/runtime semantics ad hoc.

## 12. Metrics

Self-improvement experiments report at least:

- verified task quality / error rate;
- regression and transfer performance;
- latency, compute and monetary cost;
- context size / cache effects where relevant;
- abstention/escalation behavior;
- intervention frequency;
- rollback frequency and recovery time;
- applicability/OOD coverage;
- human or verifier correction rate;
- complexity/maintenance burden of the new representation.

A lower-cost challenger that silently narrows coverage is not a win.

## 13. Relationship to MiCode

CX-16 should ingest MiCode v0.5+ supervisor, working-set, composition, semantic-alignment and shadow records as first-party self-application evidence. MiCode build-loop runs should expose the same component revision and effective-input receipts so replay can compare policy changes against real coding outcomes.

## 14. Acceptance gates

**G29-observable:** every eligible self-applied cognitive component produces a versioned operation record linked to effective input, downstream outcome/evidence and component revision; opaque production-only decisions are not promotion eligible.

**G29-kernel-boundary:** candidate self-improvements cannot alter capability enforcement, protected admission/evaluation ownership, locked-suite identity, verifier authority, provenance or rollback semantics through the candidate path.

**G29-improvement-intent:** every self-change candidate originates from an explicit immutable ImprovementIntent with incumbent identity, hypothesis, protected invariants, required evidence and rollback target.

**G29-replay-equivalence:** matched replay comparisons bind to the same state/candidate/effective-input contracts or explicitly record why exact equivalence is impossible; incomparable runs cannot be reported as direct improvements.

**G29-shadow-no-effect:** shadow challengers cannot execute tools, alter active context, steer workers, change completion state or mutate persistent production state.

**G29-independent-eval:** no component or direct successor can be the sole evaluator/admitter of its own improvement claim; protected evidence is computed outside the candidate implementation.

**G29-low-risk-envelope:** any automatic promotion is limited to an explicitly allowlisted policy class with bounded authority, independent gates, canary scope and immediate rollback; default remains manual/protected admission.

**G29-rollback:** every activated self-improvement has an immutable previous-good target and tested recovery path; monitoring can demote without consulting the candidate being removed.

**G29-lineage:** incumbent, candidate, dataset/suite revisions, experiment, shadow runs, admission decision, activation and rollback events form one durable lineage graph.

**G29-no-metric-gaming:** candidates cannot change their own success metric, evaluation population, corpus role or evidence threshold inside the evaluated change; such changes require a separate governance proposal.

**G29-self-hosting:** at least one MiCode/Axon internal component is exercised through the complete observe→replay→shadow→admit→rollback lifecycle before higher-impact self-improvement is enabled.

**G29-crystallization:** a promoted cheaper representation demonstrates preserved applicability and verified utility against the incumbent, including defined fallback behavior for uncovered/OOD cases.

**G29-new-primitive:** a proposed cognitive primitive has typed semantics, interpreter/lowering behavior, authority boundaries, replay encoding and cross-family evidence before becoming part of the Cortex vocabulary.

**G29-data-separation:** training/curriculum episodes, development selection, calibration, locked tests and transfer suites retain corpus-role lineage throughout self-improvement; production feedback cannot silently contaminate protected evaluation.

## 15. First build slice

Instrument three low-risk internal decisions in MiCode/Axon—model routing, working-set selection and semantic rule/skill selection—with `CognitiveOperationRecord`. Run at least two offline challenger policies over frozen episodes, then one live shadow challenger. No behavior may auto-promote in the first slice. Produce an `ImprovementIntent`, replay evidence, shadow evidence, protected evaluation result and tested rollback receipt for one candidate before enabling any bounded automation.

## 16. Generalization beyond Axon itself

CX-29 governs platform self-application. CX-30 applies the same observe→challenge→replay→shadow→admit→rollback structure to arbitrary owner-approved repositories, while CX-31 governs transfer between projects. The three scopes are deliberately separate so platform self-improvement, project-local improvement and cross-project generalization cannot silently share evaluation populations or authority.
