---
id: CX-27
title: "Semantic Supervisor Plane: independent semantic oversight around active cognitive workers"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-04", "CX-10", "CX-11", "CX-19", "CX-21"]
first_stage: M2
implementation_evidence: []
---

# CX-27 — Semantic Supervisor Plane

## 1. Purpose

Cortex needs an independent semantic observer around long-running workers, planners and build loops. The worker that proposes actions or claims completion should not be the only component judging whether progress is meaningful, the work has drifted, verification is needed, or human input is required.

The Semantic Supervisor Plane (SSP) evaluates compact, bounded evidence from the active task and emits typed assessments. A separate deterministic policy maps those assessments to permitted interventions. The supervisor never gains tool authority merely because it can classify a situation.

The basic shape is:

```text
Worker / planner / build loop
        │
        ├── events, diffs, tests, evidence, intent state
        ▼
Semantic Supervisor
        │ typed assessments only
        ▼
Deterministic intervention policy
        │
        ├── CONTINUE
        ├── STEER
        ├── REQUEST_VERIFY
        ├── HOLD
        ├── STOP_RETRY
        ├── ESCALATE
        └── PROPOSE_FINISH
```

`PROPOSE_FINISH` is not `VerifiedComplete`. Protected verification remains authoritative.

## 2. Assessment vector

The first coding-world profile should support independently versioned questions such as:

```text
SupervisorAssessment {
  progress_probability,
  stuck_probability,
  off_track_probability,
  intent_drift_probability,
  rule_drift_probability,
  implementation_complete_probability,
  requirements_satisfied_probability,
  tests_sufficient_probability,
  needs_verification_probability,
  needs_human_probability,
  ready_to_finish_probability,
  evidence_refs[],
  observation_digest,
  definition_revision,
  backend_provenance
}
```

The exact set is profile-specific. The contract is that each assessment states what evidence was available, which semantic definition was used, and which probability source produced the value.

## 3. Observation boundary

Supervisor observations are compact and bounded. They may include:

- approved Intent IR and unresolved clauses;
- current AIR node / worker phase;
- recent worker actions and failures;
- changed files, bounded diffs and compiler/test diagnostics;
- protected verification summaries, but not hidden verifier internals;
- prior supervisor assessments/interventions;
- elapsed time, attempt counts and resource budget state;
- applicable project rules or capability-policy summaries.

The supervisor must not dump unbounded repository state into a model merely to obtain a verdict. CX-28 owns working-set selection and receipts.

## 4. Independence

Independence means more than a different prompt. For protected use, the supervisor must preserve at least these separations:

- worker-generated self-assessments are evidence, not supervisor truth;
- completion authority stays with CX-21/CX-01 protected verification;
- permission/effect authority stays with CX-03 and the runtime;
- supervisor semantic definitions and thresholds are versioned independently from the worker;
- a worker cannot rewrite the supervisor's exam or acceptance thresholds during the task;
- protected tasks may use a different model/backend or frozen definition revision when appropriate.

## 5. Intervention policy

The learned component returns assessments. Deterministic policy decides what can happen.

Example ordering:

```text
needs_human / authority conflict
→ ESCALATE

known deterministic hazard
→ HOLD through existing authority path

repeated off-track or stuck after grace
→ STOP_RETRY

likely drift / missing evidence
→ STEER or REQUEST_VERIFY

likely complete
→ PROPOSE_FINISH

otherwise
→ CONTINUE
```

The policy must include hysteresis/grace periods where repeated evaluations could otherwise oscillate. A single noisy score must not repeatedly stop/start the same worker.

## 6. Steering rather than immediate stopping

For recoverable semantic problems, the default experiment should compare `STEER` before `STOP_RETRY`. The steering packet is constrained to the observed issue and relevant intent/evidence references; it does not grant new authority or silently rewrite the task.

Every intervention records whether the worker recovered, ignored the steering, regressed, required verification, or eventually escalated. These outcomes become labeled experience under CX-10.

## 7. Multi-timescale supervision

Supervision need not run after every token or tool call. Schedules may include:

- event-triggered assessments after edits, failures, tests or explicit completion proposals;
- debounced periodic assessments during long quiet work;
- immediate deterministic guards for known hazardous patterns;
- final pre-verification assessment before a completion proposal.

CX-06/CX-22 may later specialize the frequency/routing policy based on cost and observed value.

## 8. Learning and replay

A supervisor episode records:

```text
SupervisorDecisionRecord {
  intent_ref,
  worker_state_ref,
  observation_digest,
  assessment_vector,
  intervention,
  intervention_policy_revision,
  subsequent_worker_events[],
  verification_outcome?,
  human_outcome?,
  counterfactual_eligible,
  cost_latency
}
```

Replay should support holding the worker state fixed while swapping semantic definitions, thresholds or backends. Historical traces do not prove that an unchosen intervention would have worked; direct resettable experiments are preferred when practical.

## 9. MiCode bridge

MiCode is the first rich software-world environment for this plane. CX-16 should accept MiCode supervisor records for shadow experiments, but Axon must re-apply its own task, authority, corpus-role and evaluation contracts. MiCode supervisor conclusions are not Axon admission evidence by themselves.

## 10. Relationship to other specs

- CX-19 provides the approved intent contract and unresolved-clause state.
- CX-21 owns protected completion and whole-system benchmark evidence.
- CX-03 owns actual execution authority.
- CX-28 supplies bounded working sets for supervisor observations.
- CX-20 evaluates semantic definitions, backends and intervention policies.
- CX-22 may crystallize recurring supervision into cheaper/specialized policies.
- CX-11 owns promotion of any reusable supervisor artifact.

## 11. Acceptance gates

**G27-no-authority:** supervisor outputs cannot directly execute tools, widen capability grants, modify Intent IR, or create `VerifiedComplete`; every intervention routes through existing deterministic authority paths.

**G27-independent:** protected supervision uses immutable task/definition identities and does not accept worker-authored changes to its own criteria or thresholds during the evaluated run.

**G27-evidence-bound:** every assessment is bound to an observation digest and evidence refs; stale assessments are rejected when the relevant worker/world state has changed.

**G27-deterministic-policy:** identical normalized assessments plus policy revision and runtime state yield the same intervention; model prose cannot bypass the policy mapping.

**G27-steer-hysteresis:** recoverable drift/stuck cases support a bounded steer/grace path and cannot oscillate indefinitely between interventions without explicit attempt limits.

**G27-completion-separation:** `ready_to_finish` or `PROPOSE_FINISH` never counts as completion evidence; CX-21 protected verification can reject it without supervisor override.

**G27-bounded-observation:** supervisor requests obey registered size/redaction/provenance limits and produce an effective-input receipt; hidden verifier material and secrets are not silently included.

**G27-failure-safe:** unavailable/malformed supervisor output cannot silently grant more authority or mark work complete; configured fallback is deterministic and visible.

## 12. First build slice

Run the supervisor in shadow mode over resettable coding episodes. Measure whether it detects known stuck/off-track/premature-finish fixtures and whether interventions would have been permitted, without changing worker behavior. Only after shadow evidence should steering be enabled behind a feature gate; stopping/retrying remains later and more conservative.


## v0.13 supervisor self-application
Supervisor definitions/policies are themselves CX-29 optimization targets. A supervisor may generate evidence about another worker, but a supervisor candidate cannot be solely evaluated or admitted by itself or its incumbent.
