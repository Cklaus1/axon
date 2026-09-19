# Axon Reflex backend bakeoff

## Purpose

This build packet turns the System-One/Jev ecosystem review into a controlled Axon experiment. The goal is **not** to clone a vendor implementation or assume one neural architecture is correct. The goal is to freeze one Axon Reflex decision ABI, plug multiple backend families into it, and let protected task/evaluation evidence determine which backend deserves continued investment.

The ecosystem examples supplied by the user are treated as implementation leads, not reproduced evidence. Their benchmark, calibration, latency, and training claims must be rerun on Axon workloads before adoption.

## Stable Reflex ABI first

The runtime contract is backend-neutral. A backend receives one immutable state reference plus one or more typed finite questions and returns typed answers. It does not receive execution authority.

Required question families for the initial bakeoff:

- `Choice<T>` — one value from a runtime-supplied finite candidate set.
- `BinaryDecision` — internal Axon form for yes/no; a TypeSafe compatibility adapter may map this to/from `Noul` without making vendor terminology part of the core ABI.
- `OrdinalScore` — one bounded rubric category with explicit level meanings.
- `Rank<T>` — optional experimental family after Choice conformance; not required for the first safe repair loop.

Open-ended text/code is **not** a Reflex answer. It belongs to `Generate`. Tool execution is **not** a Reflex answer. It belongs to the executor after capability/freshness validation.

## Backend families

Run the same question corpus through at least these four families:

| Family | Definition | Implementation pattern | Probability semantics |
|---|---|---|---|
| A — Generative adapter | Ordinary LLM produces a constrained typed answer, optionally generated numeric confidence/probabilities | Official System-One-adapter-style pattern | Generated estimate or unavailable; never relabel as native logits |
| B — Direct option logits | Open model scores bounded labels/options directly | OpenJev-style label/logit implementation | Native option logits transformed under a declared normalization; not automatically calibrated correctness probability |
| C — Sequence scorer | Compute `P(option_text | state, question)` or a declared sequence score for every candidate | open-jev sequence-probability pattern | Sequence likelihood / normalized sequence score with explicit length/tokenization treatment |
| D — Learned decision head | Shared encoder or option-conditioned scorer trained for variable runtime candidate sets | Jevlike/ModernBERT-style experimental scorer | Model score/probability with separate empirical calibration artifact |

A fifth provider-native TypeSafe/Jev backend can be added as a comparison when available, but the core ABI and evaluation do not depend on it.

## Dynamic candidate-set requirement

The fundamental unit is not a fixed classification label. It is:

> score an arbitrary, runtime-generated, finite set of typed semantic possibilities conditioned on the current state.

A coding task may expose symbols `[sym:a3, sym:f9, sym:44]` today and a different set tomorrow. Backends must consume candidate IDs/descriptions at inference time, preserve opaque IDs, and avoid learning task authority from human-facing names alone.

Candidate-set metadata recorded for every request:

- ordered candidate IDs and descriptions;
- candidate-manifest digest;
- cardinality and token lengths;
- construction/retrieval policy version;
- branch condition, if conditional;
- candidate-order seed/permutation when under sensitivity testing.

## Required destructive controls

Every backend evaluation includes controls that test whether the model actually uses the state and semantics rather than priors or labels:

1. correct state;
2. shuffled state from another task in the same domain;
3. empty/minimal state;
4. wrong repository/project state;
5. randomized candidate order;
6. stable candidates with renamed opaque IDs;
7. irrelevant-state injection;
8. stale-state snapshot;
9. adversarial/distractor candidate set;
10. candidate omission followed by the permitted scope-expansion path.

A backend that retains nearly unchanged performance under destructive state shuffling is flagged as state-insensitive and cannot be promoted merely on ordinary accuracy.

## Shared-state / prefix-cache benchmark

Speculative questions benchmark both ordinary parallel calls and shared-state inference:

```text
parallel API calls:
  model(state, q1)
  model(state, q2)
  model(state, q3)

shared-state inference:
  encode/prefill(state) once
      ├─ q1 + options
      ├─ q2 + options
      └─ q3 + options
```

Record separately state encoding/prefill time, incremental question time, incremental option time, p50/p95 end-to-end latency, backend-only latency, KV/cache memory, token/compute cost, total speculative work/discarded work, and quality for the complete selected action packet.

“One request” is not treated as evidence of “one neural forward pass.” Shared-prefix caching is an optimization claim that requires measurement under the exact model/tokenizer/prompt/candidate schema.

## Decision provenance

Every answer record carries backend family/adapter version, exact model/weights/provider revision, tokenizer/quantization/runtime revision, state digest, question digest, candidate-manifest digest and ordered options, genuine raw backend scores where exposed, derived probabilities where computed, probability/uncertainty provenance class, calibration artifact ID if applicable, selected answer/abstention, latency/usage/cost/cache mode, and eventual independently verified outcome reference.

Do not synthesize raw logits for a backend that does not expose them.

## Acceptance decision

The bakeoff does not choose a universal winner. It can select different approved backends by question family/domain if the routing policy and calibration artifacts support that scope.

Continue investment only when the backend shows useful task-level selective performance after charging all state construction, inference, calibration, and fallback cost. Failure to beat the simple baseline is a valid result. Before escalating to a new architecture, inspect observation completeness, candidate recall, destructive controls, grouped-split leakage, label quality, tokenization/option semantics, calibration, and baseline strength.

## v0.3 conformance prerequisite and feasible-family rule

A backend enters scored comparison only after passing the feature subset it claims in [REFLEX_CONFORMANCE](REFLEX_CONFORMANCE.md) and completing [DEPENDENCY_ADOPTION](DEPENDENCY_ADOPTION.md) for the intended use. Backend-specific context/candidate limits are recorded rather than normalized away. Effective input, retries and transformations are charged and traced.

The M2 bakeoff compares all **feasible conforming** families. A learned head is desirable as an experiment when data/resources permit but is not mandatory to decide whether learned-head research should continue. `NotEvaluated` is distinct from `Failed` and does not block the safe coding loop.

Add candidate-search controls: flat scoring, retrieval+rereanking, and—where cardinality requires it—hierarchical or bounded beam selection. Report catalog recall and no-suitable-option detection before top-1 decision quality.

## v0.6 additional architecture arms

The model-family research matrix now explicitly includes independent option scoring, pointer/option-conditioned scoring and listwise candidate interaction. Causal/bidirectional and dense/sparse-MoE are optional follow-on hypotheses only. Report state-encode/prefill, per-question and per-candidate incremental costs separately. Candidate ordering and candidate-set composition are experimental factors, not nuisance variables to discard.

## v0.7 Kev-style executable reference arm

Add a concrete **Kev-style reference arm** to the bakeoff after the first eligible coding corpus exists: small pretrained causal backbone, lightweight adapter, exact sibling-question isolation through a branch mask, listwise/pointer readout over runtime candidates, no autoregressive answer decoding. This arm is intentionally small enough to train repeatedly while we study data/representation/transfer.

Compare it against the existing generative, direct-logit, sequence-scoring, independent option-scoring and other listwise arms under the same canonical decision encoding and candidate compiler.

Required ablations include: backbone size; frozen versus adapted backbone; pointer/listwise head versus independent scoring; question packing versus separate inference; canonical-order only versus shuffled-order training; permutation-consistency loss; distractor/absence augmentation; and state-observation variants. The purpose is to identify the bottleneck, not crown a preselected architecture.

A model that matches a reference backend on familiar tasks but collapses on repository/task-family transfer does not pass the Cortex model gate, even if latency is excellent.
