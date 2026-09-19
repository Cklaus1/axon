---
id: CX-20
title: "Reflex Research Lab: reproducible decision-model experimentation and admission evidence"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-05", "CX-06", "CX-10", "CX-11", "CX-14"]
first_stage: M2
implementation_evidence: []
---

# CX-20 — Reflex Research Lab

## 1. Purpose

Axon needs a permanent research environment for decision-model work that is stronger than a collection of scripts and notebooks. The Reflex Research Lab is the controlled experimentation substrate for designing, training, evaluating, comparing, reproducing, and promoting Reflex models and runtimes.

The Lab exists to answer a narrow question with high evidentiary quality:

> Which decision-model/runtime configuration most improves verified Axon task performance under fixed authority, task, data, compute, cost, latency, and evaluation contracts?

The Lab is not itself an execution authority, deployment gate, or production inference service. It produces research artifacts and evidence for independent admission under CX-11.

## 2. Decisive fork

Treat model research as a governed experimental system, not an informal sequence of training runs.

Every promoted conclusion must be reconstructible from:

- source code and exact commit;
- dataset/suite manifests;
- model and tokenizer identities;
- canonical decision encoding version;
- training configuration;
- environment/hardware manifest;
- random seeds where applicable;
- evaluation configuration;
- raw result artifacts;
- registered acceptance criteria;
- admission decision.

A result that cannot be reconstructed is informative exploration but not promotion evidence.

## 3. Scope

The Lab owns research infrastructure for:

1. backend-neutral Reflex conformance;
2. decision-corpus construction and lineage;
3. training/evaluation partitions;
4. small learned Reflex pilots;
5. backend and architecture bakeoffs;
6. calibration and selective-routing experiments;
7. shared-state / branch-isolation studies;
8. candidate-set and option-order robustness;
9. coding-transfer evaluation;
10. runtime performance experiments;
11. reproducibility and experiment provenance;
12. artifact registry and research-to-admission handoff.

The Lab does not own:

- production execution authority;
- final release/promotion decisions;
- user intent resolution;
- Axon capability policy;
- proof-kernel semantics;
- hidden-test authoring by the model under evaluation;
- changing its own acceptance thresholds after observing locked-test outcomes.

## 4. Lab architecture

```text
Research question
      ↓
Experiment registration
      ↓
Frozen suite + data manifest
      ↓
Candidate configuration
      ↓
Training / inference run
      ↓
Conformance + mechanism tests
      ↓
Calibration + task evaluation
      ↓
Transfer / robustness evaluation
      ↓
Cost / latency / memory accounting
      ↓
Evidence bundle
      ↓
Independent CX-11 admission
```

The Lab is split conceptually into six services. They may begin as modules/scripts and need not become separate crates.

### 4.1 Suite Registry

Owns immutable versioned research suites with:

- training partition;
- calibration partition;
- development partition;
- locked test partition;
- transfer partitions;
- contamination metadata;
- task-family and repository-family grouping;
- hashes and revision pins;
- licensing/usage metadata;
- candidate-generation policy version.

### 4.2 Experiment Registry

Owns preregistered experiment records:

```text
ExperimentSpec {
    id
    hypothesis
    candidate_configs
    fixed_controls
    suite_version
    primary_metrics
    secondary_metrics
    stop_rules
    compute_budget
    allowed_hyperparameter_space
    promotion_claim_scope
}
```

An experiment's primary metric, comparison baseline, and stopping rule are frozen before reading the locked test.

### 4.3 Runner

Runs local or remote trials while recording:

- code commit/hash;
- dirty-tree status;
- dependency lock hash;
- model/base revision;
- tokenizer revision;
- canonical encoder version;
- hardware/device;
- precision/quantization;
- environment/container image;
- seed;
- wall-clock and compute time;
- peak memory;
- cost;
- complete training configuration.

The runner refuses to overwrite an existing run identity.

### 4.4 Evaluator

Runs the same registered evaluation code for all candidates and baselines. It reports at minimum:

- accuracy / task-specific quality;
- NLL;
- Brier score where applicable;
- ECE and reliability curves;
- selective risk vs coverage;
- abstention behavior;
- candidate-order sensitivity;
- state-destructive controls;
- question-isolation conformance;
- packed-vs-separate equivalence/tolerance;
- candidate-absence behavior;
- latency decomposition;
- memory/compute cost;
- end-to-end verified task impact;
- Coding Transfer Frontier.

Evaluation failures remain in the denominator unless the registered task contract explicitly declares them ineligible.

### 4.5 Artifact Registry

Stores immutable references to:

- model adapters/weights;
- readout heads;
- calibrators;
- tokenizer/config;
- encoder/renderer version;
- model card;
- training log;
- evaluation report;
- suite hashes;
- experiment registration;
- provenance chain;
- applicability envelope;
- revocation/deoptimization state.

A research alias may point to a current candidate, but promotion evidence always references immutable artifact IDs.

### 4.6 Evidence Bundler

Produces a machine-readable and human-reviewable bundle suitable for CX-11 admission. The bundle states exactly what is claimed and what is not.

Example:

```text
Claim:
  improves symbol-selection VUC on unseen Rust repositories
  under candidate sets <= 128 and state <= 4k tokens

Does not claim:
  cross-language generalization
  calibrated correctness on new workflows
  permission to execute selected actions
```

## 5. Research partitions and anti-leakage rules

Random example splits are insufficient for claims of coding generalization.

The Lab supports at least these grouping levels:

1. episode-level;
2. commit-level;
3. repository-level;
4. repository-family-level;
5. task-family-level;
6. language/ecosystem-level.

For general coding claims, repository-level or stronger holdouts are required.

No ordinary model-selection process may read the locked test. Transfer suites remain transfer suites until a new suite version explicitly promotes them into training data. Once transfer examples are used for training, prior transfer claims are retired for the new artifact.

## 6. Canonical decision encoding

Training, evaluation, serving, and semantic replay use the same versioned canonical decision encoder unless an experiment explicitly studies an alternative encoding.

The canonical encoding records:

- state representation version;
- question type and ID;
- candidate IDs and descriptions;
- candidate generation policy;
- candidate order policy and order digest;
- dependency type (`Independent`, `ConditionallyRelevant`, `AnswerDependent`);
- control candidates (`NONE`, `OBSERVE_MORE`, `ESCALATE`, `BLOCKED`) where allowed;
- truncation/projection receipts;
- structured field rendering rules;
- reserved-token escaping/sanitization.

Train/serve skew is a gate failure, not a footnote.

## 7. Reflex model families under test

The Lab must support comparing, without privileging one in advance:

- structured-output generative adapters;
- direct option-logit baselines;
- sequence-likelihood scorers;
- independent option scorers;
- pointer/option-conditioned scorers;
- listwise candidate models;
- Kev-style shared-state/block-causal/pointer models;
- later specialized architectures when justified by measured bottlenecks.

Causal vs bidirectional, dense vs sparse/MoE, and proprietary-style training methods are hypotheses, not assumptions.

## 8. Kev-style reference arm

A Kev-style reference arm is maintained because it demonstrates a low-cost working mechanism:

```text
shared state
   ↓
block-causal isolated branches
   ↓
listwise option visibility
   ↓
pointer readout
   ↓
direct probability distribution
```

The Lab may reproduce this mechanism using a small pinned open backbone and adapter/head. The purpose is not to clone Kev or Jev as a product. It is to provide a stable research reference against which Axon-specific improvements are measured.

The reference arm must be evaluated on coding tasks before any coding-performance claim is made.

## 9. Transfer-first research policy

In-distribution quality is necessary but not sufficient.

Every candidate intended for general Axon coding use reports:

```text
same-task/same-distribution
same-ecosystem/unseen-repo
unseen-repo-family
unseen-task-family
unseen-language/ecosystem (when available)
```

The **Coding Transfer Frontier** is the furthest registered distribution shift at which the candidate satisfies the preregistered quality, selective-risk, coverage, cost, and latency envelope.

A candidate can be well calibrated but too inaccurate to be useful. Calibration and intelligence are reported separately.

## 10. Mechanism tests

Every learned Reflex candidate runs mechanism tests independent of task accuracy.

### 10.1 Question isolation

Compare:

- Q alone;
- Q + unrelated sibling;
- Q + adversarial sibling;
- Q + conflicting sibling;
- Q + many irrelevant siblings.

Backends claiming branch isolation must meet the registered numerical tolerance.

### 10.2 Packed vs separate

Compare packed multi-question inference against separate single-question inference. Record semantic deltas and speedup. Any intentional non-equivalence must be declared as a model/policy change.

### 10.3 Candidate permutation

Measure:

- argmax flip rate;
- probability spread;
- NLL/Brier changes;
- selective-routing changes.

Compare augmentation-only, canonical-order-only, and permutation-consistency objectives.

### 10.4 Candidate absence

Fixtures deliberately omit the correct ordinary action. The model must choose an allowed control outcome or abstain; normalization cannot force an arbitrary semantic action.

### 10.5 Candidate distractors / IIA-like stress

Add irrelevant or near-duplicate candidates and measure changes in relative odds and final behavior.

### 10.6 Boundary-forgery / encoding attacks

Try reserved-token injection, fake candidate delimiters, prompt-role injection, candidate-ID spoofing, and malformed structured content. The encoder must preserve structural boundaries.

### 10.7 State dependence

Compare correct state, shuffled state, empty state, wrong repository, stale state, and irrelevant-context injection. A candidate whose performance barely changes under state destruction cannot claim meaningful state-conditioned intelligence.

## 11. Calibration and routing research

Calibration artifacts are first-class versioned outputs. They bind to:

- model/backend revision;
- quantization/precision/runtime;
- canonical encoder version;
- task/question family;
- candidate-generation and ordering policy;
- state projection/truncation policy;
- calibration dataset/version.

The Lab reports raw distributions and calibrated distributions separately.

Generic `confidence` is presentation metadata, not authority. No threshold can grant permission or certify DONE.

## 12. Training objectives

Initial research uses simple measurable objectives before exotic training methods:

- cross-entropy / NLL over candidates;
- Brier score reporting;
- ranked probability score for ordinal decisions;
- optional permutation-consistency regularization;
- optional abstention/control-candidate objectives;
- later task-utility objectives only with protected end-to-end evaluation.

Any RL-like training program must define its reward, data source, leakage protections, off-policy assumptions, and protected evaluation separately. `RLCD` is not treated as a sufficient algorithm description.

## 13. Data eligibility

Only data eligible under CX-10 may train production-candidate models.

Weak labels, teacher labels, model disagreement examples, synthetic examples, imported MiCode episodes, and external repository observations carry explicit lineage and eligibility class.

A successful action does not automatically label all alternative actions as wrong. Multiple acceptable actions are supported. Unobserved counterfactuals remain unknown unless evaluated in a resettable environment or supported by a registered off-policy method and its assumptions.

## 14. Research loops

### 14.1 Inner experiment loop

```text
registered hypothesis
→ run bounded trial
→ evaluate
→ inspect failure mode
→ one bounded configuration change
→ rerun development suite
```

Stop on budget, preregistered futility criterion, or completion of the experiment matrix.

### 14.2 Outer research loop

```text
model family / runtime hypothesis
→ learning curve
→ mechanism tests
→ calibration
→ transfer evaluation
→ system-level task evaluation
→ retain / reject / refine
```

### 14.3 Meta-research loop

```text
which bottleneck dominates?
  representation?
  data?
  architecture?
  calibration?
  serving?
  candidate generation?
      ↓
register next experiment
```

The meta-loop may propose a new experiment or metric. It may not rewrite a completed experiment's exam or locked results.

## 15. Research-to-production boundary

The Lab cannot activate a candidate.

Promotion requires CX-11 and the applicable CX-01/CX-05/CX-06 gates. At minimum:

1. immutable artifact identity;
2. conformance pass;
3. calibration/applicability evidence;
4. transfer evidence appropriate to claim scope;
5. end-to-end verified task benefit;
6. cost/latency/resource accounting;
7. rollback/deoptimization plan;
8. independent admission record.

A model may be admitted only for a narrow decision family even if it fails broader generalization.

## 16. Security and isolation

Training and evaluation workers are untrusted relative to the Axon admission boundary.

The Lab must support:

- isolated workers;
- no access to production secrets by default;
- no write access to locked-test data from ordinary training jobs;
- bounded external network access according to dataset/model acquisition policy;
- immutable run outputs;
- explicit dependency/model adoption under `DEPENDENCY_ADOPTION.md`;
- no production authority inherited from the artifact being evaluated.

A candidate model never evaluates its own promotion evidence without an independent verifier reproducing the result.

## 17. Reproducibility classes

Every result declares one of:

- **Exact** — deterministic or bitwise reproduced under pinned environment;
- **Numerically equivalent** — within registered tolerance;
- **Statistically reproduced** — repeated experiment meets registered interval/effect criterion;
- **Observed once** — exploratory only, not promotion evidence.

Nondeterministic GPU/model behavior is not mislabeled as exact reproducibility.

## 18. Acceptance gates

**G20-registry:** suites, experiments, runs, artifacts, and evidence bundles have immutable IDs and complete lineage. Missing lineage blocks promotion use.

**G20-locked-test:** ordinary training/model-selection jobs cannot access the locked test. Test access creates a release-evaluation record and cannot be silently repeated for tuning.

**G20-canonical-encoding:** the same versioned encoder/rendering contract is used for training, evaluation, serving, and replay, or an explicit distribution-shift experiment owns the divergence.

**G20-reproduce:** one reference run is reproduced under its declared reproducibility class from pinned source/config/data/model artifacts.

**G20-mechanism:** every learned Reflex release candidate passes registered isolation, packed/separate, permutation, absence, boundary-forgery, and state-dependence tests.

**G20-transfer:** any general coding claim reports held-out repository/task-family transfer and Coding Transfer Frontier evidence. Same-repository random splits cannot satisfy this gate.

**G20-calibration:** calibration evidence is partition-correct, version-bound, and reported separately from accuracy/intelligence. Locked test is not used to fit calibration.

**G20-system-impact:** a candidate intended for production use improves a preregistered end-to-end verified task objective or provides a separately justified capability under fixed authority/evidence conditions.

**G20-budget:** every experiment obeys registered compute/cost/time bounds; missing cost/usage data is `Unknown`, not zero.

**G20-admission-boundary:** the Lab produces evidence only. Activation requires CX-11 independent admission.

## 19. Build slices

### Slice 0 — Research registry

Implement suite/experiment/run/artifact schemas and immutable manifests.

### Slice 1 — Frozen suite harness

Support train/calibration/dev/locked-test/transfer partitions with hashes and grouping metadata.

### Slice 2 — Canonical decision encoder

Use the same encoder in training, evaluation, serving fixtures, and semantic replay.

### Slice 3 — Reference baselines

Run structured-output, direct-logit/sequence-scoring, and Kev-style reference arms on one bounded decision family.

### Slice 4 — Mechanism suite

Implement isolation, packing, permutation, absence, distractor, boundary-forgery, and state-destruction tests.

### Slice 5 — Calibration/transfer lab

Fit calibrators only on calibration data; evaluate dev, locked test, and transfer separately; produce Coding Transfer Frontier.

### Slice 6 — Remote runner

Add optional GPU/container execution with source/suite/model hashes and cost accounting. Local execution remains supported for smoke tests.

### Slice 7 — Admission handoff

Generate CX-11 evidence bundles and rehearse promotion/rollback of a non-production candidate.

## 20. First forcing-function experiment

Use one narrow Axon coding decision family, such as **relevant-symbol selection** or **next-verification-check selection**.

Data:

- MiCode/Axon eligible episodes;
- synthetic resettable tasks;
- repository-grouped train/dev/test;
- at least one unseen-repository transfer partition.

Compare:

1. simple heuristic/rule baseline;
2. structured-output strong-model adapter;
3. direct-logit or sequence-scoring baseline;
4. small Kev-style learned model.

Measure:

- task accuracy;
- candidate recall;
- NLL/Brier/ECE;
- selective risk/coverage;
- permutation stability;
- question isolation;
- latency/cost;
- end-to-end verified coding impact;
- transfer degradation.

A successful first experiment does not mean “build the proprietary Reflex model.” It means the Lab can produce trustworthy comparative evidence and identify the next bottleneck.

## 21. Non-goals

The Lab does not promise:

- Jev parity;
- frontier-model-equivalent transfer;
- a novel foundation model;
- universal calibration;
- autonomous model promotion;
- production authority;
- general intelligence.

The Lab is successful when it makes model research falsifiable, reproducible, comparable, and safe to feed into Axon's independent admission process.

## v0.9 specialization and learned-function research

The Lab now supports a broader **Cognitive Specialization** experiment class. Registered trials may compare general Reflex, specialized encoder/classifier, candidate-token scorer, pointer/listwise scorer, typed neural program, deterministic rule/template and THINK/GENERATE fallback under one protected evaluator. Model research and data curation remain separate from activation; CX-22 interprets specialization eligibility/ROI and CX-23 supplies neural-program runtime contracts.

For shared-base learned artifacts, the Artifact Registry additionally pins compiler revision, base-model revision, adapter/program digest, prompt/renderer digest and runtime-manifest version. A mutable alias never replaces immutable experiment identity.


## v0.10 perception and serving research families

The Lab now admits semantic extractors, semantic proposition matchers, generated-probability adapters, direct next-token scorers, listwise/pointer models and encoder classifiers as separate experiment families. It also evaluates learner/sampler candidates from CX-25. Wire/API compatibility alone never collapses their probability semantics or evidence classes.

## 15. v0.11 semantic alignment loop

Model weights are only one part of a decision function. The Lab therefore treats the semantic definition itself as an experimental artifact.

Independently version and evaluate:

- question/instruction wording;
- criteria and option descriptions;
- state projection;
- candidate-generation and ordering policy;
- monolithic versus decomposed questions;
- deterministic composition rules;
- model/runtime configuration.

An uncertainty-driven labeling loop may select ambiguous examples plus a random audit sample for human or protected-verifier labeling, propose semantic-definition revisions, and show a semantic diff. Development-score improvement never activates the revision automatically; locked/transfer evidence and CX-11 admission remain required for production use.

Question decomposition is explicitly testable:

```text
one broad judgment
→ narrower typed judgments
→ deterministic composition
```

The Lab must record whether gains came from better semantics, better candidates, better representation, calibration, or model weights. A semantic-definition improvement must not be reported as a model architecture improvement.

**G20-definition-lineage:** every question/criteria/decomposition/candidate-policy revision used in an experiment has immutable identity and complete example/result lineage; changing semantics without a new identity invalidates the comparison.

**G20-active-labeling:** uncertainty-selected examples are accompanied by a random audit sample and purpose/contamination labels; active selection alone cannot define the reported population error rate.

**G20-decomposition:** a proposed monolithic-to-decomposed decision change is evaluated against the original under the same task/authority/verifier contract, and the deterministic composition rule is versioned and replayable.

**G20-no-auto-promote:** no semantic-definition, prompt, criteria, decomposition or candidate-policy revision becomes active merely because development/training score rises; independent admission remains required.

## v0.12 supervisor and working-set experiments

The lab may compare supervisor semantic definitions, intervention thresholds/policies, working-set selectors, semantic-GC decisions, model routes and cascade policies under fixed worker/task states. Active/uncertainty sampling must retain random-audit coverage. Context-reduction experiments report protected-clause retention and effective-input receipts in addition to model metrics.


## v0.13 self-application experiments
The lab treats internal policy/model/question/candidate-construction revisions as experiment artifacts. It must support matched replay and shadow comparisons for model routing, working-set selection, semantic definitions and other CX-29 targets without automatic promotion from development score.
