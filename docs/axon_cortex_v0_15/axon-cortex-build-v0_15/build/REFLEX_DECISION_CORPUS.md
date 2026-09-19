# Axon Reflex decision corpus and evaluation construction

## Purpose

Build the empirical substrate for Axon Reflex before custom architecture work. The corpus contains **typed bounded judgments with independently checkable outcomes**, not generic chat transcripts and not raw replay logs copied wholesale into training.

## Unit of data

```text
DecisionExample {
  task_family
  episode_ref
  observation/state digest + eligible state projection
  typed question schema
  runtime-generated candidate set
  selected/teacher answer, if any
  verified outcome label or label status
  backend/model provenance
  split/group lineage
}
```

Keep `VerifiedOutcome`, `HumanReviewed`, `TeacherOnly`, `Weak`, `Delayed`, and `Unknown` labels distinct. Teacher agreement is reported separately from real task outcomes.

## Grouped split policy

Prevent leakage by grouping related examples before splitting. At minimum group by repository/task family, bug seed/template, code lineage, mutation family, and near-duplicate semantic structure when known. Derived variants from one underlying task stay in the same split.

Keep protected final-audit families inaccessible to the builder/model-training process. Similarity checks supplement provenance grouping; they do not replace it.

## Decision families to collect first

Prioritize judgments already present in the safe coding loop:

- operation selection: inspect/search/edit/check/escalate/done/blocked;
- target selection from dynamic symbols/tests/scopes;
- whether more observation is required;
- whether a candidate is stale/inapplicable;
- which check is likely to be informative;
- failure localization across observer/reflex/world-model/planner/generator/executor/verifier;
- decomposition signals such as capability expansion, trusted-code touch, external-effect change, test weakening, unchecked input, and invariant impact.

Each family needs a downstream use, verifier/reviewed label source, and baseline.

## Error-attribution dataset

Move attribution earlier because controlled self-improvement depends on localizing errors. For failed episodes capture candidate responsibility across `OBSERVER`, `RETRIEVAL`, `REFLEX`, `WORLD_MODEL`, `PLANNER`, `GENERATOR`, `EXECUTOR`, `VERIFIER`, `MULTI_CAUSAL`, or `UNKNOWN`.

Attribution remains a hypothesis unless substitution/intervention evidence supports it. Do not globally retrain all pillars because one classifier emits a confident cause.

## Question-decomposition dataset

Store both monolithic and decomposed formulations when System Two or humans discover a useful decomposition.

```text
monolithic: "is this patch safe?"

decomposed signals:
  expands capabilities?
  touches trusted code?
  changes external effects?
  weakens a test?
  introduces unchecked input?
  alters an invariant?
```

The deterministic composition rule is a versioned artifact. Evaluate whether decomposition improves verified downstream decisions, calibration, latency, cost, or transfer. Successful decompositions become candidates for crystallization into Reflex templates/rules.

## Corpus quality controls

For every corpus version record source/episode lineage and redaction version, label provenance/revision, candidate-construction policy, task-family/group split manifest, class/candidate-cardinality distribution, duplicate/near-duplicate audit, state/option length distributions, OOD/protected families, revoked examples/reasons, and eligible uses: training, calibration, development evaluation, or protected evaluation.

Raw Axon replay journals may contain secrets and are **not** automatically eligible training data. Export only purpose-scoped redacted projections through CX-10 lineage rules.

## Learning-curve protocol

Train/evaluate at multiple verified-data sizes. Compare simple priors/rules, lightweight classifier/ranker, direct-logit open model, sequence scorer, learned option-conditioned head, and generative strong-model adapter.

Report grouped held-out curves, calibration, coverage, latency/cost, destructive state controls, and real downstream task impact. The purpose is to discover the data/architecture regime, not to prove that a custom model must exist.

## v0.3 labels, candidate absence and behavior policy

Corpus rows distinguish candidate recall from option selection. Include examples where no supplied candidate is suitable, where further observation would reveal a candidate, and where a suitable target exists but is not authorized.

Label targets are explicit: semantic class, acceptable action set, observed action outcome, comparative action value, or task completion. Multiple acceptable next actions are representable. An unchosen candidate is not automatically a negative. Alternative action outcomes are `Unknown` unless independently evaluated or supported by a declared estimator.

Record the behavior-policy version and, when known, action-selection probability separately from model correctness/confidence. Group/split by repository/task/bug lineage to prevent near-duplicate leakage.

## v0.7 coding transfer and Kev-style training records

The corpus now maintains five explicit partitions for Reflex model research: `train`, `calibration`, `development`, `locked_test` and `transfer`. Split construction is repository-aware before example-level balancing. Derived commits, forks, generated siblings and near-duplicate bug families remain in one group unless a reviewed transfer experiment explicitly says otherwise.

Minimum transfer ladder for coding research:

1. unseen episodes in a known repository;
2. unseen repositories in a known ecosystem/language;
3. unseen repository families/frameworks;
4. unseen task families;
5. cross-language transfer where the candidate/observation schema remains meaningful.

Every record stores the canonical decision-encoding version and exact candidate order. Training/evaluation/serving/replay all invoke the same canonical encoder implementation where possible.

Add explicit negative candidate-set records: correct candidate absent; more observation required; escalation required; all ordinary candidates distractors. These examples use registered typed control candidates rather than marking the least-bad ordinary candidate as correct.

For permutation research, preserve paired examples with stable semantic candidate IDs and multiple randomized orders. The paired lineage allows augmentation and permutation-consistency objectives without leaking variants across train/calibration/dev/test/transfer partitions.

The first learned Axon Reflex pilot may start when a clean coding corpus is large enough to produce a measurable learning curve; data quality and transfer coverage, not a fixed example count, determine expansion.
