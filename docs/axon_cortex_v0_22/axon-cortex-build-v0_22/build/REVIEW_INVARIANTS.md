> **v0.21 Review-invariant amendment.** Use [research integration](RESEARCH_INTEGRATION_V021.md), [source upgrade](UPGRADE_V021.md) and [v0.21 bootstrap](BOOTSTRAP_PROMPT_V021.md) for the current additive delta. Historical profiles below retain their original version scope. Package statuses do not reset live source evidence.

# Cross-cutting review and adversarial invariants

These rules amend the supplied Cortex design after the two review passes. They apply to existing components as well as CX-36. This file consolidates implementation obligations; owning specs define their gates.

## Assurance is several separate checks

`FormatValid` checks decoding, schema and resource limits. `SourceBound` checks that a value came from the specified source and projection. `SemanticallyValidated` checks a named task-specific property. `Authorized` checks current grants. `Admitted` checks the independent release policy. None implies another. A signature authenticates an issuer and subject; it does not prove the subject correct. A valid digest is integrity, not trust.

`CorrectnessEstimate` names its event (for example reference-label agreement, factual extraction accuracy, or verified task success), estimator digest, fitting cohort, applicable domain and uncertainty. It is not interchangeable with option mass, teacher consensus, applicability, OOD score or validation status. Missing estimates stay unavailable. Evaluation criteria must match the named event.

## Immutable subject, detached decisions

Freeze executable bytes before protected evaluation. Evaluation, calibration and admission receipts reference the frozen subject; release decisions are detached from it. Adding an admission signature must not change the subject that was evaluated or create a self-referential digest. Every target conversion/quantization gets a new subject identity and target-specific evaluation. Active aliases resolve atomically to immutable deployment revisions.

## Candidate admission and revocation

Candidate storage is not active registry publication. Isolated inspection/inference has its own research grant. A loadable candidate does not gain production eligibility. Dispatch rechecks tenant, principal, project, input scope, artifact/target, current revocation generation and remaining budget; cached authorization is not sufficient. Expired/revoked previous-good artifacts cannot become rollback targets. During a revocation partition, protected dispatch fails closed or follows an explicit short-lived lease policy frozen by the owner.

## Effects, not names

Read-only work can disclose source, consume paid inference, write caches/logs or contend for hardware. Track state mutation, data disclosure, resource use and external effects independently. Reversibility describes a particular effect, not permission to act. Shadow means **no control or task effects**; it is not free of compute, egress or audit writes. Reserve a separate bounded shadow budget and enforce privacy before calling a shadow provider. Foreground work is protected from starvation.

## Replay, interventions and unknowns

Recorded-response replay verifies reconstruction of a recorded path. It does not demonstrate a new policy would have produced the same downstream outcome. When context/definition/candidate selection is the intervention, hold the upstream task/snapshot/catalog constant and record the **changed** effective input. A simulator or off-policy estimator never emits a realized outcome, even when validated. Realized alternate outcomes require actual controlled execution. Unknown/missing evidence is not false; censored attempts remain in denominators according to the preregistered policy.

## Bounded recursion and fallback

Every fallback is a new traced invocation with renewed hard checks but the same parent deadline and aggregate budget. Reject cycles or bound revisits explicitly; denial does not route to a more permissive provider. Meta-optimization and self-instrumentation have recursion-depth, event-volume and compute limits. The audit writer, kill latch, signer and minimal reference scheduler are not allowed to recursively depend on their own learned replacements.

## Context and semantic extraction

Pinned requirements that cannot fit cause budget/context escalation or refusal, not silent truncation. Keeping a pointer is not the same as making its text visible to the model; a receipt states the exact projection. An extractor's span proves location only; its interpretation remains a hypothesis until the relevant check validates it. Tri-state logic is used for uncertain semantic predicates: NOT Unknown = Unknown; False AND Unknown = False; True AND Unknown = Unknown; True OR Unknown = True; False OR Unknown = Unknown. Probabilistic independence is never inferred from separate questions.

## Data and evaluator protection

Raw source, prompt caches, model-native state, embeddings, derived labels, graphs, artifacts and hashes of low-entropy secrets all remain subject to disclosure/retention policy. Use permitted tombstones after deletion; do not retain a forbidden payload just to keep replay exact. Decline exact-replay claims when required data has lawfully expired.

Protected evaluation uses a durable submission ledger, predefined feedback limits and family-aware splits. Neither model input nor ordinary worker filesystem contains hidden answers or signing keys. The tested executable may see test inputs but not acquire the evaluator's authority. Human labels carry annotator/rationale/disagreement provenance; an approval is not automatically a factual gold label. Correction/retraction is a new event.

## Transfer and architecture change

Transfer dimensions are not a single ordered scale: unseen repositories, language and task families are distinct axes. T tiers (Coding Frontier) and P tiers (repository learning) require explicit claim-profile mappings; reaching one tier does not imply success on all lower-numbered or unrelated tiers. Kernel changes, language opcodes and latent state synthesis require separate governance/research; good empirical specialization cannot authorize them.

## v0.19 decision-model invariants

- raw probability/distribution ≠ empirical correctness probability;
- calibrated correctness ≠ authority or completion;
- listwise candidate interaction is allowed, so IIA is diagnostic unless a specific model contract requires it;
- packed execution ≠ semantic permission to leak across branches;
- stage-1 high-cardinality score ≠ final joint choice probability;
- outcome feedback ≠ online self-promotion;
- a secondary reverse-engineering explanation ≠ verified vendor internals.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
