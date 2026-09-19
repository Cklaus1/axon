# Loops: running Cortex, learning, and building Cortex

The previous roadmap blurred three activities: executing a task, improving a component, and changing the architecture. This protocol separates their state, budgets, evidence and write permissions. “Inner/outer/meta” describes containment and responsibility, not a requirement for infinite recursion.

## A. Runtime loop hierarchy

| Loop | Inputs | Work | Writes | Stop condition / gate |
|---|---|---|---|---|
| R1 — Inner action loop | Pinned observation, action catalog, remaining budget | Select or request context; generate concrete payload if needed; predict when supported; revalidate; execute; observe; check | Owned ephemeral workspace, action/evidence events | One action reaches Verified/Refused/Failed/Canceled/OutcomeUnknown; no hidden retries |
| R2 — Outer task loop | Goal contract, subgoals, working beliefs, R1 evidence | Plan, choose next action/experiment, evaluate progress, replan | Task working state and proposed plan | Independent goal completion; authority/evidence/budget limit; cancellation; bounded no-progress |
| R3 — Component learning loop | Eligible labeled episodes, candidate failure attribution, fixed baseline | Propose one update; train/derive; run held-out and adversarial eval | Candidate artifact and unsigned proposal; no release authority | Candidate rejected/inconclusive or submitted to independent admission |
| R4 — Portfolio/meta-learning loop | Results across components and tasks, fixed audit policy | Choose next component/experiment; test bottleneck hypotheses; propose curriculum/routing changes | Experiment plans, training curricula, candidate policy revisions | Budget exhausted, evidence insufficient, no expected useful experiment, or bounded review complete |
| R5 — Architecture governance loop | Repeated interface failures, ADRs, invariant/TCB changes | Compare architecture alternatives, migration cost, effects and evidence | Reviewed specifications and explicitly approved policy changes | Independent owner/governance decision; never self-activation |

The admission authority is outside R3/R4. Runtime capability enforcement and protected final evaluation are outside learner write permissions at every loop level.

## R1 exact protocol

Acquire current snapshot and grant references; verify parent budget; resolve schema dependencies; run Rule/Reflex/Generate/Reason as permitted; construct a concrete action artifact; obtain a scoped prediction or explicit “not supported” record once that feature is enabled; prepare; atomically revalidate; execute; record observed delta; run the registered action check; release reservations and update actual usage.

The runtime preserves a hard semantic split:

```text
DECIDE   = choose bounded semantic intent/target; no side effect
GENERATE = synthesize open-ended payload/artifact; no authority
ACT      = trusted executor validates grant + payload + freshness, then performs effect
```

A Reflex answer can never directly become a shell command, path, selector, network destination, or write. Open-ended strings from a generative backend remain untrusted payloads until the executor's typed action validator accepts them.

Invalid or stale inputs go back to observation, not to repeated execution. No side effect is performed while merely choosing a branch. An ambiguous external outcome enters reconciliation. Exact host replay serves recorded outcomes and performs no real side effect.

## R2 exact protocol

Maintain current subgoal, hypotheses, contradictions, expected next observations, progress signature and budgets. Choose between action, evidence gathering, deliberate reasoning, authorized human review or stop. Replan after meaningful new evidence, not automatically after every unchanged observation. A DONE proposal triggers protected goal checks; failure returns the evidence and consumes the same task budget.

Learning inside an episode is limited to working-state updates. Changing model weights, calibrated thresholds or gate versions creates an explicit new epoch with fresh applicability checks; it does not silently alter the running task.

## Error attribution as an early cross-loop service

Do not wait for late-stage meta-learning to begin failure localization. Every failed/blocked episode should emit candidate responsibility across Observer, Retrieval, Reflex, WorldModel, Planner, Generator, Executor, Verifier, MultiCausal, or Unknown. Attribution is a hypothesis unless supported by substitution/intervention evidence; it is used to choose the next diagnostic experiment, not to trigger broad retraining automatically.

Track whether the failure came from missing candidates, wrong state, state-insensitive decision behavior, generation, stale execution, protected-check failure, or verifier insufficiency. The Reflex corpus should retain these cases for targeted classifiers and question-decomposition experiments.

## R3 exact protocol

Select a narrow hypothesis about a component failure. Validate data eligibility and label quality. Freeze the baseline, split and evaluation policy. Propose an update in an isolated workspace. Evaluate with ablations and actual resource accounting. Submit a complete manifest to CX-11 admission. Rejected candidates and failed experiments remain in provenance; they are not omitted from progress reports.

Do not require a single certain causal attribution before experimenting. Unknown/multi-causal records can motivate targeted probes. They cannot justify changing every component at once.

## R4/R5 exact protocol

R4 allocates research attention and compute; it does not change what counts as safe or successful. It may propose a revised metric when existing evidence shows a mismatch. R5 independently reviews the proposal, reruns incumbents/challengers under both policies and records the change. Old results retain their original policy version.

This distinction prevents a system from reporting progress by lowering thresholds, selecting easier tasks, deleting failures or removing required checks.

## B. Builder loop hierarchy

| Loop | Purpose | Typical output |
|---|---|---|
| B-inner | Implement one falsifiable behavior change with red/green, negative and seam tests | Reviewable patch plus command/evidence record |
| B-outer | Integrate a vertical slice across observer, executor, model seam and verifier | Demonstrated milestone with baseline comparison and rollback |
| B-meta | Find which development assumption, interface or test is causing churn | Bounded experiment or ADR, not an unreviewed rewrite |

A builder working on the verifier cannot simultaneously serve as its release approver. Generated tests are useful development artifacts but must be reviewed against the locked requirement; tests manufactured solely to pass the new implementation do not establish correctness.

## Reflex loop instrumentation

When R1 invokes Reflex, record exact model/backend revision, state digest, question digest, ordered dynamic option set, raw score source, any derived probabilities, calibration artifact, timing/cost and eventual verified outcome. If the backend uses shared-state/prefix caching, split timing into state prefill/encoding, incremental question work and incremental option work.

The router treats `NativeOptionLogit`, `SequenceLikelihood`, `GeneratedEstimate`, `EntropyDerived`, `EnsembleEstimate`, `CalibratedEmpirical`, and `Unavailable` as different evidence origins. Calibration/authority is never inferred merely from a numeric score.

## Common loop envelope

Every loop invocation has loop ID/kind, parent ID, objective, versioned inputs, read/write scope, authority, budgets, attempt count, evidence access, progress measure, allowed updates, termination criteria and output schema. Parent budgets are reserved/charged atomically across descendants. Parent cancellation stops new child work and triggers the host's actual termination protocol.

## Example: type-lowering failure

R1 runs the authorized parity check and observes a divergence. R2 compares two hypotheses—type-map mismatch versus incorrect native layout—and chooses a targeted inspection/check. R3 later tests a specialized error-triage adapter trained on eligible verified cases. R4 may prioritize type-map work over more model inference if ablations support that bottleneck. R5 reviews an actual compiler/invariant change; the learner cannot alter reference semantics to make the divergence disappear.

## Example: misleading improvement

A Reflex candidate is cheaper but abstains on every hard task. Component latency improves, but task coverage violates the fixed policy. R3 rejects it. R4 cannot fix this by deleting those tasks. It can propose a new domain-limited deployment envelope, which is then explicitly evaluated and approved as a narrower capability.

## C. Experience federation and knowledge loops (v0.4)

Cortex now distinguishes where experience came from without giving any source a different truth standard.

| Loop | Inputs | Work | Writes | Stop condition / gate |
|---|---|---|---|---|
| E1 — Experience federation | Native Axon episodes, MiCode bundles, generated curricula | schema/identity migration, data-use eligibility, dedupe/lineage, semantic normalization | eligible evidence store; never authority registries | invalid schema/lineage/policy, unresolved required identity, or accepted evidence bundle |
| E2 — Repository knowledge loop | eligible repositories/histories/MiCode episodes | extract patterns, search counterexamples, reproduce locally, benchmark/holdout | knowledge candidates + negative knowledge | contradicted scope, unreproducible claim, inconclusive evidence, or PromotionEligible candidate |
| E3 — Crystallization loop | admitted pattern/concept/skill candidates | synthesize guarded skill/tool/library/compiler/runtime candidate, run CX-11/CX-15 evidence | candidate artifact registry only | rejected/inconclusive, active guarded artifact, or deoptimized previous-good |

The canonical timescales are: ms–seconds working state/routing; seconds–minutes planning/experiments; hours skill/tool/calibration candidates; days world-model/representation/knowledge updates; weeks+ model/compiler/library/runtime promotion and governance. Trusted verifier/admission/capability policy never silently adapts inside R1/R2.

MiCode is an E1/E2 producer and experimental consumer. A MiCode episode may be rich evidence, but its local permission outcome cannot authorize Axon effects. Imported historical actions are never replayed as real effects merely because they occurred before.

## D. Intent loops (v0.5)

| Loop | Inputs | Work | Writes | Stop condition / gate |
|---|---|---|---|---|
| I1 — Intent inner loop | natural/structured intent | parse, type/normalize, detect conflict/ambiguity, resolve/refuse, semantic render | versioned Intent IR proposal only | valid unresolved/refused/approved contract; never "retry until convenient" |
| I2 — Intent/task outer loop | approved Intent IR | lower to AIR, plan/act/observe/replan while preserving clauses | AIR/execution/evidence artifacts | independently verified completion, explicit blocked state, or budget stop |
| I3 — Improvement-intent loop | measured bottleneck/pattern/knowledge candidate | construct `ImprovementIntent`, request authority/evidence, evaluate candidate | candidate/admission artifacts | reject/inconclusive/promote through CX-11; learner cannot change its exam |
| I4 — Intent meta loop | ambiguity/rendering/schema outcomes | propose parser/schema/UX changes, benchmark on protected intent corpus | future candidate versions only | independent evaluation/admission; active episode contract is immutable |

Intent clauses are parent constraints for R1/R2. Runtime replanning can choose a different strategy or stronger verification, but cannot silently relax the approved hard constraints, authority ceiling or required evidence.
