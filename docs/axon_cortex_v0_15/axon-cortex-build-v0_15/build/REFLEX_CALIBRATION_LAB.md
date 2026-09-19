# Axon Reflex calibration and selective-routing lab

## Goal

Determine when a Reflex answer is reliable enough to replace or avoid more expensive cognition **without confusing model scores, uncertainty summaries, empirical correctness, authority, or proof**.

## Uncertainty provenance classes

Every probability-like value declares its origin. Suggested wire-level classes:

```text
NativeOptionLogit
SequenceLikelihood
GeneratedEstimate
EntropyDerived
EnsembleEstimate
CalibratedEmpirical
Unavailable
```

A value may carry more than one stage, for example native logits plus an empirical calibrator. Preserve the raw source and calibrated result separately.

`EntropyDerived` is uncertainty about a distribution, not automatically a probability that the selected answer is correct. `GeneratedEstimate` is model-authored content, not native scoring evidence. `CalibratedEmpirical` is valid only inside the artifact's declared model/schema/domain/candidate-construction envelope.

## Required metrics

At the question-family level report top-1/top-k accuracy where meaningful, NLL, Brier, ECE with binning details, reliability diagrams, selective risk vs coverage, error under abstention, subgroup/task-family/OOD performance, candidate-order sensitivity, candidate-count sensitivity, state-shuffle/wrong-state sensitivity, and latency/cost/memory including prefill/incremental timing.

At the routed system level report **Verified Utility at Coverage (VUC)** or an equivalent preregistered measure: how much workload Reflex handles at a specified independently verified error/quality envelope after fallback cost is charged.

Example question:

```text
Can Reflex handle >= X% of eligible bounded decisions while the independently
verified error rate remains <= Y and the end-to-end cost/latency improves versus
the locked baseline?
```

X and Y are task/policy-specific preregistered values, not universal constants.

## Calibration artifacts

Bind each artifact to model/weights/provider revision, tokenizer/quantization/runtime, prompt/question schema, candidate representation/construction policy, task/domain families, calibration dataset/grouping/split manifest, fitting method, validity/revocation triggers, metrics/intervals, and coverage/applicability envelope.

Any relevant model/schema/candidate-policy change invalidates or narrows the artifact until rechecked.

## Routing hierarchy

Authorization and mandatory verification come first. Within eligible actions, routing may use empirical selective risk, cost, latency, novelty, disagreement and progress. The router may choose Rule, Reflex, more Observe/Retrieve, Generate, Reason, Experiment/Predict, human review or Blocked.

No confidence value can create permission, waive a mandatory check, or certify DONE.

## Acceptance / pivot

Adopt a backend/router configuration only if it improves a preregistered task-level objective under minimum quality and coverage constraints. An all-abstain system fails coverage. A high-coverage but high-risk system fails quality. If no configuration improves economics, retain the incumbent and record the negative result.

## v0.3 coverage accounting

For every registered experiment:

- `Coverage = handled_without_escalation / eligible_decisions`.
- `SelectiveError = incorrect_handled / handled`; undefined when `handled = 0`.
- Eligibility rules and failure handling are frozen before scoring.
- Report protected task success, latency and total cost beside calibration/selective metrics.
- Any VUC-style aggregate publishes its formula/weights and cannot hide an all-abstain policy or dropped failures.

Calibration applicability binds the effective input/preprocessing receipt. Material truncation, projection, tokenizer change, candidate-policy change or question-fusion change requires revalidation.

## v0.6 calibration domain refinement

A calibration artifact is keyed by model/backend revision, effective-input policy, question family, candidate-generation policy **and candidate-order policy**. The primary learned object is a distribution; display confidence is a derived summary with formula provenance. Report NLL and Brier score alongside ECE, selective error/coverage, VUC and task-level outcomes. Reordering or materially changing candidate composition is a distribution shift unless revalidated.

## v0.7 transfer-aware calibration

Calibration and intelligence are distinct. Report calibration separately on in-distribution, repository-held-out and transfer partitions. A backend can be well calibrated while too inaccurate to be useful; low ECE never substitutes for task quality.

Define a **Coding Transfer Frontier** for each registered decision family: the furthest registered distribution shift at which the backend meets the preregistered selective-quality, coverage and compute/cost envelope. Report the whole frontier curve rather than one scalar whenever possible.

Temperature scaling or another post-hoc calibrator is fit only on the calibration partition. Development data selects configurations. The locked test remains untouched until promotion evaluation. Transfer partitions are not repurposed into training without minting a new corpus/suite version.
