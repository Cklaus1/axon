---
id: CX-14
title: "Parallel decision-model research"
status: Draft
authority: Proposed
depends_on: ["CX-05", "CX-06", "CX-10", "CX-11"]
first_stage: M5
implementation_evidence: []
---

# CX-14 — Parallel decision-model research

## Intent and source basis

Investigate a learned Axon Reflex model only after the workload/interface and admission path exist. S2 pp.7–15 distinguishes runtime engineering from a custom foundation model. W1/W2 are vendor interface descriptions, and W5 is a lead for bounded parallel decoding—not proof of equivalent training or calibration. See [SOURCES](../SOURCES.md).

## Decisive fork

Run a learning-curve and inference-bottleneck program, not a mandatory progression toward a particular neural design. Begin a pilot when eligible labeled data exists; there is no arbitrary million-example prerequisite. A custom architecture is pursued only when it addresses a measured quality/cost/latency limit.

## Candidate model stages

R0: existing model with constrained schemas or candidate scoring; establish exact probabilities available versus missing. R1: small standard model/classifier or adapter trained for selected tasks. R2: shared state representation with question- and candidate-conditioned scoring. R3: specialized parallel inference/training only if R2 and simpler baselines leave a demonstrated bottleneck.

These are experiment options, not successively guaranteed releases. One model does not need to handle every task family. A mixture of simple calibrated specialists may be the better measured result.

## Model interface

Input includes structured/unstructured authorized state, typed questions, candidate IDs plus semantic descriptions, field dependencies and schema hash. Outputs conform to CX-05. Dynamic target cardinality requires candidate-conditioned scoring or another explicitly supported mechanism; fixed class heads alone cannot represent arbitrary runtime symbol sets.

A shared encoder/decoder must preserve question isolation where promised and declare dependencies where needed. Memory and compute scale with context, questions and candidates; avoid constant-time claims. Quantization, prefix caching, batching and candidate-token encoding are independently benchmarked and calibration-tested.

## Training data and objectives

Training examples carry approved outcome labels or clearly marked weak teacher labels, task family, candidate-generation policy, model/prompt versions, support weighting and contamination lineage. Teacher disagreement can identify useful examples, not establish the true answer. Preserve abstentions and failed episodes, not only successes.

Begin with appropriate supervised objectives and empirical calibration. A calibration-focused training objective must be stated precisely and evaluated with held-out proper scoring and selective task risk. The term RLCD is not an implementation specification. No proprietary method is assumed reproducible from its name.

Maintain held-out schemas, candidate phrasings/orderings, task families and repositories to test generalization beyond training labels. Control for label leakage through descriptive IDs. Data quantity decisions follow learning curves with uncertainty; include training and serving total cost.

## Inference experiment matrix

Sweep context length, question count, candidate cardinality, token-prefix ambiguity, cold/warm cache, batch size, concurrent load, quantization and device type. Record prefill, decision scoring, queue, serialization and host overhead separately. Compare with a no-probability label-only backend only as a separately identified interface, not an apples-to-apples speed victory.

Initial hardware is the configured available host; no required GPU purchase is encoded here. Report exact hardware/software and measured memory. Remote inference policy and data projection remain CX-10/CX-13 requirements.

## Acceptance gates

**G14-pilot:** a small approved dataset produces a reproducible learning curve against the strongest relevant simple baseline; invalid labels or contaminated splits block the experiment.

**G14-quality:** task and calibration non-inferiority under CX-01 policy, with per-family/OOD results and abstention coverage, passes before cost savings justify release.

**G14-parallel:** real context/cardinality/load benchmarks show whether specialized parallel inference helps; semantic and active-branch conformance match CX-05.

**G14-version:** quantized/retrained/tokenizer-changed artifacts cannot reuse stale calibration or approvals.

**G14-release:** a successful research artifact still passes independent CX-11 admission and rollback rehearsal; a notebook metric cannot self-activate a model.

## Build slices and exclusions

Start with one routing/check-selection task and actual labels. Benchmark first; then train a small adapter; then inspect whether shared-state computation is useful. Frontier-equivalent capability, generalized calibration and a novel foundation model are research questions, not promised milestones.

## v0.3 research sequencing

The M2 bakeoff requires conformance machinery and comparison among feasible existing backend families; it does not require a learned-head implementation in order to justify learned-head research. A lightweight learned option scorer is optional in M2 when the corpus/resources exist. Expanded learned-head/shared-encoder/RLCD-like work remains a later research branch justified by measured workload/serving gaps. `NotEvaluated` is distinct from `Failed`.

## v0.6 model hypotheses from Jev architecture probing

Treat the Jev architecture analysis as hypothesis generation, not source code or a disclosed training recipe. The research matrix now compares at least these bounded-decision families when justified by workload evidence:

1. **Independent option scorer** — score each option conditioned on state/question, then normalize.
2. **Slot-head classifier** — shared representation plus fixed/variable decision heads where appropriate.
3. **Pointer/option-conditioned scorer** — encode candidate objects and select among runtime options.
4. **Listwise option model** — jointly represent the complete candidate list before producing a distribution.

Listwise interaction is a first-class hypothesis because candidate composition/order may change relative odds. It is not assumed correct a priori; compare it against simpler scorers under matched candidate sets and compute budgets.

Causal-decoder versus bidirectional encoder and dense versus sparse-MoE are later research branches only. No build slice may claim the external article identified Jev's exact backbone. Sparse MoE is considered only after a measured dense-model serving bottleneck.

Training pilots optimize/report proper scoring objectives such as negative log likelihood and Brier score in addition to accuracy. Post-hoc calibration remains separate. 'RLCD reproduction' is not a milestone name unless a reproducible public algorithm is actually pinned; this package uses **Outcome-Calibrated Decision Training** as the generic research program.

**G14-listwise:** compare independent, pointer/option-conditioned and listwise candidates on dynamic option sets, option-order perturbations and held-out candidate compositions. Any listwise advantage must survive compute-matched and task-level evaluation.

**G14-training-score:** a training pilot publishes learning curves for accuracy, NLL, Brier, calibration/selective risk and downstream verified task impact; no single metric is sufficient for promotion.

## v0.7 Kev reference implementation findings

`jaredpalmer/kev` is now an executable reference for the Jev-style mechanism rather than only a behavioral reconstruction. Its released research prototype uses a frozen small causal LM, LoRA, a block-causal shared-state/question-branch mask, and a pointer/listwise readout over runtime options. The important update for Cortex is **not** that this exact model should be copied. It is that the mechanism is cheap enough to prototype early, while the harder research problem is transfer beyond the training distribution.

Kev's public results show a large gap between in-source performance and out-of-source transfer. Cortex therefore treats cross-repository/task-family transfer as a primary model gate rather than an optional appendix. A small Axon/MiCode coding model pilot may begin once a clean eligible corpus exists; no million-example threshold is required.

Required v0.7 model experiments:

1. **Kev-style reference arm** — small pretrained backbone + adapter + block-causal isolated branches + pointer/listwise readout.
2. **Transfer-first evaluation** — repository-, repository-family-, task-family- and eventually language-held-out suites; in-repository random splits are insufficient evidence for broad coding use.
3. **Permutation robustness training** — compare augmentation-only, permutation-consistency KL and canonical-order-only controls. Candidate order remains recorded even if robustness improves.
4. **Absent-candidate training** — include explicit `NONE`, `OBSERVE_MORE` and `ESCALATE` control candidates where semantically valid, plus distractor candidates. Softmax over an inadequate set must not force a false semantic action.
5. **Canonical train/serve/replay encoding** — one versioned renderer/encoder owns question/candidate formatting. Any training-specific rendering path is a conformance failure unless explicitly modeled as a distribution shift.
6. **Frozen research suites** — separate training, calibration, development, locked test and transfer partitions with revision/hash provenance. Locked test access is reserved for promoted candidates.
7. **Coding Transfer Frontier** — report quality as distance from the training distribution under fixed quality/coverage/compute constraints, not only aggregate held-out accuracy.

Kev's implementation is an experimental baseline, not proof of TypeSafe's proprietary training method or general coding intelligence. The initial public Kev checkpoint is narrow and explicitly not trained on coding. Cortex must reproduce all mechanism claims on the exact adopted commit/model and evaluate on Axon workloads.

**G14-transfer-frontier:** a candidate that claims general coding utility reports repository-family and task-family held-out results, selective coverage and uncertainty under a frozen transfer suite. Same-repository/random-example performance cannot satisfy this gate.

**G14-permutation-training:** permutation robustness is measured before and after any invariance objective. Improvements must not hide accuracy/calibration regressions, and exact candidate order remains in replay/provenance.

**G14-absent-candidate:** on fixtures where the correct semantic action is absent, the model may select only a registered absence/control outcome (`NONE`, `OBSERVE_MORE`, `ESCALATE` as policy permits), never an arbitrary ordinary candidate forced by normalization.

**G14-canonical-encoding:** training, evaluation, live inference and semantic replay use the same versioned canonical decision encoding or record an explicit transformation with separate calibration. A divergent hidden renderer blocks promotion.

**G14-locked-test:** ordinary training/model-selection workers cannot read the locked-test partition; promoted candidates are evaluated once under the registered release process, with suite/model/code hashes recorded.
