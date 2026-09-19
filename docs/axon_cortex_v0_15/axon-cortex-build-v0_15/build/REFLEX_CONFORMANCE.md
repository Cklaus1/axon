# Axon Reflex conformance contract

**Status:** Draft normative build contract for v0.3.

## Purpose

A Reflex backend is conforming only when it preserves Axon question semantics, reports what input it actually evaluated, exposes its backend limits, and fails visibly when it cannot provide required evidence. Matching the shape of a TypeSafe/Jev response is not sufficient.

This contract covers provider-native System-One services, ordinary LLM adapters, direct-logit scorers, sequence scorers, and learned decision heads. It does **not** grant execution authority.

## Primitive semantics

V0 distinguishes the result families rather than coercing them to a universal `selected + confidence` record.

- **Choice<T>** returns a selected candidate or abstention plus, when available, a complete distribution over the supplied candidates. Candidate order is part of the request manifest even when a backend claims order invariance.
- **BinaryProbability** returns an explicit probability for the positive event when the backend provides one. It is not silently rounded into a Boolean and no separate confidence field is invented.
- **OrdinalDistribution** returns the full distribution over ordered rubric levels plus a derived expectation/mean only when that derivation is declared. A fractional mean must never be silently rounded to a category.
- **Rank<T>** is experimental and requires an explicit ranking/scoring contract before admission.

Open-ended code or text is `Generate`, not Reflex. Execution is `Act`, not Reflex.

## Batch contract

Requests may contain several typed questions. Results are keyed by stable `QuestionId`, never matched by array position.

```text
DecisionBatchResult {
  request_id,
  results: Map<QuestionId, QuestionResult>,
  attempt_records[],
  aggregate_usage,
  backend_manifest_ref,
  effective_input_receipt_ref
}

QuestionResult =
    ChoiceResult
  | BinaryProbabilityResult
  | OrdinalDistributionResult
  | Abstained
  | Failed
```

Duplicate question IDs refuse the request. Unknown returned IDs are errors. Missing required active-branch results block action construction. A failed unused speculative branch may be tolerated only when the versioned batch policy explicitly says so. Out-of-order backend completion is permitted because identity is explicit.

## Probability and score provenance

A backend reports only evidence it actually exposes. Allowed origins include:

- `ProviderReportedDistribution`
- `NativeOptionLogit`
- `SequenceLikelihood`
- `GeneratedEstimate`
- `EntropyDerived`
- `EnsembleEstimate`
- `Unavailable`

Empirical calibration is a separate artifact and never replaces raw origin. Entropy-derived confidence is not relabeled as correctness probability. Generated numeric confidence remains generated evidence until calibrated against protected outcomes.

## Backend feature negotiation

Every adapter supplies a versioned `BackendFeatureManifest` before dispatch:

```text
BackendFeatureManifest {
  adapter_version, backend_family, model_identity,
  supported_question_types[],
  supported_input_modalities[],
  maximum_questions?, maximum_candidates?,
  context_limits?, candidate_text_limits?,
  available_score_forms[],
  question_isolation_mode,
  preprocessing_policy_ref,
  cancellation_semantics, usage_reporting,
  immutable_model_revision_available
}
```

AIR validates a request against the effective backend manifest before inference. A fallback is a new dispatch and must independently pass feature validation. Provider limits are not copied into AIR as universal constants.

## Effective-input receipt

The trace records both what Cortex intended to provide and what the backend actually evaluated.

```text
EffectiveInputReceipt {
  submitted_observation_digest,
  effective_input_digest,
  preprocessing_manifest,
  tokenizer_revision?,
  omission_or_truncation_report[],
  effective_question_digests,
  effective_candidate_digests
}
```

Silent truncation is forbidden. When evidence is omitted because of context or candidate limits, the adapter must refuse or produce an explicit authorized projection receipt. Calibration established on complete inputs is inapplicable to silently altered inputs.

## Trusted prompt-role boundary

Only the trusted adapter constructs privileged model instructions. Repository text, logs, retrieved documents, recorded conversations, candidate descriptions, and source comments remain untrusted data even if they contain strings such as `system`, `assistant`, tool calls, or prompt delimiters.

An adapter that accepts chat-shaped state must preserve this boundary rather than promoting untrusted role labels to privileged messages.

## Question isolation and fusion

Conformance includes tests for:

1. `Q` alone;
2. `Q` plus an unrelated question;
3. `Q` plus a contradictory/adversarial unused question;
4. reordered questions;
5. different speculative branches;
6. different cache/prefix modes.

A backend may claim exact question isolation only for a pinned mode that demonstrates it. Otherwise the property is measured empirically. Combining two model calls into one joint prompt is a model/policy change unless the backend can demonstrate that the declared computation is preserved.

## Dynamic candidate conformance

Tests cover empty sets, singleton sets, many candidates, multi-token candidates, Unicode, prefix-sharing options, renamed opaque IDs, reordered candidates, duplicated semantic descriptions, and a correct option near the backend limit.

The adapter must distinguish:

- no suitable supplied option;
- more observation/candidate expansion may reveal an option;
- a suitable option exists but is unauthorized;
- inference failed or was unsupported.

## Retry, normalization and usage accounting

Every backend attempt is represented in `attempt_records`, including corrective retries and transient retries. Probability repair/normalization, candidate remapping, truncation, and fallback transformations are explicit transformations with versions. They consume the same parent budget.

Failures stay in evaluation denominators. An adapter cannot improve apparent reliability by dropping malformed/failed calls from results.

## Required conformance fixtures

The implementation must ship recorded fixtures for at least:

- TypeSafe-compatible Choice/Noul/Score mappings;
- fractional ordinal expectation with distinct underlying distributions;
- provider result without logits;
- missing active-branch result;
- failed unused speculative result;
- duplicate and unknown question IDs;
- candidate limit exceeded;
- decisive evidence beyond context limit;
- fake `system` messages inside repository text;
- question-interference/adversarial batch;
- multi-token candidate scoring;
- normalization repair plus retry budget;
- immutable and non-immutable model identity cases.

## Admission rule

A backend can join the bakeoff only after these fixtures pass for the capabilities it claims. Unsupported features are recorded as unsupported, not simulated. Conformance does not imply calibration, task quality, safety, or authority.

## v0.6 shared-state, isolation and ordering fixtures

Add fixtures for `StateHandle` identity/expiry/tenant isolation; Q1-alone versus unrelated/contradictory/adversarial/100-sibling batches; candidate order permutations; derived confidence summaries versus primary distributions; and backend claims about actual shared-state reuse. A backend may conform without reusable KV/state, but it must not claim the optimization when it only repeats full inference.
