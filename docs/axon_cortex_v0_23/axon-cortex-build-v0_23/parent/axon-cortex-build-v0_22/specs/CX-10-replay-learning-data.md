---
id: CX-10
title: "Replay, learning data and error attribution"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-01", "CX-02"]
first_stage: M1
implementation_evidence: []
---

# CX-10 — Replay, learning data and error attribution

## Intent and source basis

Make every executed step auditable and every eligible outcome learnable without treating every trace as training material. S1 pp.15–16 and 94 describes host replay and secret-bearing journals; S2 pp.44–48 describes per-pillar learning and attribution. See [SOURCES](../SOURCES.md).

## Decisive fork

Use one linked event lineage with separate storage/access projections: restricted raw replay, redacted review, policy-approved learning export and sealed evaluation. Do not create four unrelated truth stores or train indiscriminately on raw host journals.

## Event contract

Events include run/episode/action IDs, parent causal IDs, sequence/epoch, manifest/version references, principal handle, observation/state/question/candidate-set digests, ordered dynamic candidate manifest, requested/effective backend family and exact model/tokenizer/runtime revisions, raw-score/probability provenance, calibration artifact, prediction, chosen action, grant reference, budget reservation/usage, prefill/incremental/total timing where available, executor state, observed outcome, verifier evidence and later label/attribution revisions.

Write-ahead events precede non-idempotent effects. Completion/receipt and verification are separate events. Outcome labels distinguish ObservedVerified, ObservedUnverified, WeakTeacherLabel, Simulated, Delayed, Censored, and Unknown. A verifier label identifies its artifact and contract, not just a boolean column. Correction creates a linked revision rather than overwriting prior evidence.

## Replay semantics

Reuse the documented AxonHost seam where possible. Record model/tool replies, nondeterministic clocks/RNG, selected actions and completion order sufficient for the supported replay tier. Exact replay means serving recorded responses, not making a fresh remote model call with the same seed.

Declared tiers: exact serialized host/model replay; environment reset plus rerun with tolerance-defined comparisons; and statistical reproducibility across fresh runs. Concurrency and GPU nondeterminism must not be mislabeled byte-identical. When a needed input was never recorded, return unsupported/incomplete replay.

Replaying an external write returns its recorded receipt and does not repeat the effect. Divergence includes changed inputs, missing/excess events or unconsumed required events. Diagnostic metadata identifies the first supported divergence point.

## Data eligibility and privacy

A `DatasetManifest` records source episodes and revisions, legal/owner authority for use, intended purpose, retention, sensitivity, permitted recipients/regions where configured, redaction transform version, task/repository/bug-lineage grouping, split membership, contamination/near-duplicate checks, candidate-construction policy and deletion/deactivation lineage. No eligibility metadata means not eligible for training.

Raw journals require restricted access and protected storage; hexadecimal encoding or hashing is not encryption or anonymity. Redacted review artifacts must not expose secrets through summaries, small-domain hashes or error messages. Remote inference receives only an authorized projection of observations; local audit permission does not imply remote disclosure permission.

Separate tenant datasets and inference caches. Honor retention/deletion policy while preserving a minimal permitted audit tombstone; do not promise deletion from already trained weights without a defined retraining/unlearning procedure. Quarantine model artifacts trained on later-invalidated data until policy resolves their use.

## Error attribution

An `Attribution` contains candidate causes, evidence, confidence/probability origin, tested interventions, implicated component versions and Unknown. Multiple causes may coexist: truncated observation and a miscalibrated router, for example. Do not force a single label.

Use component substitution, replay and controlled ablations to confirm causes. Preserve the distinction between “the generator changed” and “the generator caused the failure.” Update one component per experiment by default; bundled updates need factorial/interaction tests or a justified coordinated protocol.

## Acceptance gates

**G10-replay:** recorded task execution repeats without filesystem/network/model side effects; changed arguments cause visible divergence.

**G10-crash:** an episode interrupted between action and receipt remains OutcomeUnknown until reconciled; no fictitious success label enters training.

**G10-secrets:** seeded secrets in source, environment, prompts and errors remain unavailable in redacted review and learning export; raw-journal access is separately controlled.

**G10-lineage:** every training row points to permitted evidence and split lineage; weak/simulated labels cannot be silently upgraded to real outcomes.

**G10-attribution:** a deliberately multi-causal failure can retain multiple candidate causes/Unknown; a confirmed substitution updates the attribution with supporting evidence.

## Build slices and exclusions

Implement the common event schema and raw/redacted split during the first vertical slice. Add learning export before any training. Sophisticated attribution models follow basic replay; no global automatic retraining in v0.

## v0.3 decision-label and behavior-policy lineage

A successful executed action is not automatically the uniquely correct decision. Learning exports distinguish `semantic_class`, `acceptable_action_set`, `observed_action_outcome`, `comparative_action_value`, and `task_completion`. Unchosen alternatives remain unknown unless independently evaluated or supported by a declared estimator.

Each decision record stores the behavior-policy version and, when known, the probability with which the controller selected the executed action. This selection probability is not the Reflex probability that the action is correct. Historical-trace evaluation of a new policy must state its off-policy assumptions and action coverage; resettable software fixtures should prefer controlled alternative execution when practical.

Multiple acceptable next actions are supported. The corpus must not turn an unchosen but valid inspection/test into a negative example merely because a prior controller chose something else.

Replay also records submitted-versus-effective model input, backend feature manifest, adapter transformations/retries and resolved transitive model/tokenizer/encoder identities where available.

## v0.4 experience federation

Learning/replay manifests add `source_system`, source schema/version, bridge-migration version and source data-use lineage. MiCode/external repository records remain ineligible by default until CX-16/CX-17 policy checks pass. Historical action outcomes can support semantic replay and world-model learning but never authorize repeating the action. Self, external and generated experience retain separate source labels for ablations and contamination analysis even after normalization.

## v0.3 decision-label gates formalized in the spec

**G10-multi-valid:** two independently validated useful next actions can coexist in learning data; the unchosen valid action is not auto-labeled negative.

**G10-behavior-policy:** behavior-policy version and selection probability, when known, are separate from Reflex correctness probability; unobserved alternatives remain unknown absent explicit evidence.


## v0.13 self-application records
CX-10 stores CX-29 `CognitiveOperationRecord` and self-application lineage as durable experience. Outcome labels must come from downstream evidence/verification or remain unknown; component self-report is not retrospective ground truth.

## v0.16 review amendment — Bounded retention and evidence eligibility

All raw/derived traces, cache state, graph payloads and low-entropy hashes are subject to policy. Lawful deletion may leave a permitted tombstone and downgrade replay support. Human corrections are typed events, not retroactive edits. Counterfactual proposals and unchosen actions do not acquire observed labels merely because they share an episode with a successful action.

**G10-retention-replay:** removing disallowed retained input invalidates exact-replay claims and dependent use as required, without fabricating a replacement or leaking deleted data through derived exports.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE integration

ACE P10 keeps recorded reply replay, fresh inference, resettable-world rerun and counterfactual simulation distinct. Raw receipts do not imply training rights; retain corpus/family/retention restrictions and exact missingness. See [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md).

## v0.20 amendment — schema decisions and lightweight Reflex specialization

Add permission-scoped durable teacher-label jobs and grouped dataset preparation through existing record owners, as specified in the [lightweight profile](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md). Cache keys bind task/teacher-resolved-version/endpoint/effective input, tenant/project/principal, grant/data-use/retention, corpus/partition/evaluator epoch and budget. ID joins are exact and unique; conflicts require adjudication. Preserve teacher labels, authenticated human corrections, factual outcome evidence and approval as different provenance. Unknown outcomes are not positive labels. Related/duplicated episodes cannot cross protected partition boundaries; grouping does not prove statistical independence.

**G10-teacher-job-scope:** Teacher jobs and cache resumes bind principal/project/data-use/retention/grant, exact task/input, resolved teacher/endpoint, corpus/partition/evaluator epoch and budget; torn/duplicate/unknown requests cannot duplicate publication or leak protected labels.

**G10-grouped-role-isolation:** Exact and related input/episode/repository groups are isolated across train/selection/calibration/acceptance/test; protected-label cache reuse, correlated pseudo-replication and adaptive holdout reuse cannot masquerade as independent evidence.

**G10-label-adjudication:** Human correction, teacher distribution, approval, tool Success and verified outcome remain separately authenticated evidence; conflicting duplicates require adjudication and ambiguous effects never become factual success labels.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G10-r21-usage-partitions:** Usage categories are disjoint and price-revision-bound; every billable call, retry, embedding, judge and cancelled branch joins by stable task/attempt/call IDs and is counted exactly once.

**G10-r21-usage-unknown:** Missing or partial usage is explicit unknown with a reserved ceiling; all assigned tasks, failures and abstentions remain in quality/cost reporting rather than disappearing from a successes-only denominator.

**G10-r21-research-replay:** Replay consumes recorded decision/provider, route, context and speculative outcomes without fresh model calls; principal, candidate, epoch and projection mismatches produce explicit divergence.

**G10-r21-evidence-integrity:** Episode content hashes are not claimed as externally anchored append-only proof; durable ordering and tamper detection use existing audit/recording authority and preserve claimed-versus-observed event distinctions.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G10-r22-trial-identity:** Repeated runs of one task and arm use distinct TrialIds; transport retry reuses only the identical OperationId/input binding, while an authorized new execution uses a new AttemptId. Same semantic task ID never deduplicates a fresh trial.

**G10-r22-all-attempts:** Every model/tool/check attempt including retries, abandoned speculation, refusals, failures and unresolved outcomes remains attributable to a unique trial and included in denominator and cost policy.

**G10-r22-full-task-cost:** Cost comparisons include uncached input, cached input, output, inference/encoder, checks, failed/retried/cancelled work and execution charges under pinned price schedules; cache hits and GPU time are not assumed free.

**G10-r22-cohort-denominator:** All assigned tasks are retained in paired outcomes; failures are not omitted from average-cost reporting and successes alone cannot redefine the comparison denominator or experiment population.

**G10-r22-eligibility-projection:** Missing data-use, tenant, provenance, model/evaluator revision or corpus-role facts block learning export; redaction retains a hash-bound omission/projection record rather than claiming unchanged canonical content.
