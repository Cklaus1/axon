# Axon Reflex — Kev-style reference pilot

## Purpose

Build the first small learned Axon Reflex backend as a **reference experiment**, not as the production default. The target is to reproduce the useful mechanism demonstrated by `jaredpalmer/kev` on Axon/MiCode coding decisions, then determine whether the bottleneck is model architecture, software-state representation, training data, candidate construction or transfer.

## Entry conditions

- CX-05 Reflex ABI and conformance fixtures exist.
- B21 decision corpus has independently eligible labels and repository-aware lineage.
- B58 Reflex Runtime can supply immutable state/candidate manifests.
- B63 has frozen `train`, `calibration`, `development`, `locked_test` and `transfer` partitions.
- One canonical decision encoder is pinned for training/evaluation/serving/replay.

## Reference architecture

Use a small pretrained causal backbone as the initial arm, with:

1. shared state prefix;
2. isolated question branches under a block-causal mask;
3. branch-local position policy;
4. runtime candidate list visible to the decision token;
5. pointer/listwise readout from the decision representation to candidate representations;
6. softmax distribution per typed question;
7. no autoregressive answer generation.

Start with lightweight adaptation (for example LoRA or an equivalent bounded adapter) plus a small readout head. Exact backbone, size and adaptation method are experiment parameters, not architecture invariants.

## Coding decision families

Initial families should be independently checkable and naturally finite:

- operation selection (`INSPECT`, `SEARCH`, `EDIT`, `CHECK`, `ESCALATE`, `DONE`, `BLOCKED`);
- symbol/test/scope target selection;
- check-selection / information-gain proxy;
- failure localization;
- `OBSERVE_MORE` / candidate-absence detection;
- escalation to THINK.

Do not begin with open-ended patch generation; that remains `GENERATE`.

## Training controls

Compare at least:

- canonical order only;
- option-order shuffle augmentation;
- permutation-consistency objective;
- distractor augmentation;
- typed absence/control augmentation;
- pointer/listwise head versus a simpler independent scorer;
- one or more backbone sizes if the first learning curve justifies it.

For ordered `Score`-like tasks, compare ordinary cross-entropy with an order-aware proper-scoring term. Keep post-hoc calibration separate from model training.

## Evaluation hierarchy

Report separately:

1. in-repository development;
2. unseen repositories in the same language/ecosystem;
3. unseen repository families and bug/task families;
4. cross-language transfer where the observation/candidate schema is still meaningful;
5. selective quality/coverage after routing/abstention;
6. task-level verified utility and total compute/cost.

Do not use the locked test for model selection. Calibration is fit on the calibration split only. Transfer data does not silently become training data without a new suite version.

## Mechanism/conformance tests

- packed versus separate questions;
- sibling-question isolation;
- boundary/delimiter forgery;
- candidate-order permutations;
- irrelevant-option insertion;
- missing-candidate / `NONE` / `OBSERVE_MORE` behavior;
- effective-input truncation/omission receipt;
- state-shuffle and wrong-state controls.

## Stop / pivot rules

Stop or pivot the architecture when one of these holds:

- simple baselines match the learned model under the same transfer/task budget;
- familiar-split gains do not transfer after representation/data improvements;
- order sensitivity remains above the registered action-risk envelope;
- the observer/candidate compiler, rather than model capacity, dominates errors;
- inference economics do not beat the incumbent after fallback cost.

A negative result is useful: it localizes the next research target.

## Promotion boundary

A successful pilot remains a research artifact until CX-11 independent admission passes. It cannot change authority, waive verification, access the locked test during tuning or activate itself.
