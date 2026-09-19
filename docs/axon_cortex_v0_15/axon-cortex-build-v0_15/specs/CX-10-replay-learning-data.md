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
