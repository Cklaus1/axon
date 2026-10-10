# Reflex Runtime — shared-state execution and branch scheduler

**Status:** Draft build guide. This document does not claim a Reflex runtime is implemented.

## Purpose

Separate the long-lived Axon Reflex **runtime** from any particular Reflex **model**. The runtime evaluates many typed decision branches over one immutable software/world observation while preserving AIR dependency semantics, provenance, calibration domain, budgets and authority separation.

## Target architecture

```text
Software/World Observation
        |
        v
EffectiveInputReceipt
        |
        v
State Encoder / Prefix Cache
        |
        v
StateHandle
   |      |      |
   v      v      v
 Q1     Q2      Q3
indep   cond.   answer-dependent -> next stage
   |      |
   +------+
      |
DecisionBatchResult
      |
branch/joint validation
      |
Action proposal (still untrusted)
```

## Runtime responsibilities

- Canonicalize and authorize the observation projection before model dispatch.
- Bind `StateHandle` to state/model/tokenizer/adapter/preprocessing/tenant/expiry.
- Schedule `Independent`, `ConditionallyRelevant`, and `AnswerDependent` questions correctly.
- Pack dynamic candidates with deterministic construction and order manifests.
- Preserve opaque candidate IDs independently of human labels.
- Track prefill/state-encode, incremental-question, candidate and total latency/cost/memory separately.
- Enforce cancellation and parent budgets across all speculative branches and retries.
- Map provider responses by `QuestionId`; partial failures follow explicit batch policy.
- Never let speculative results directly invoke tools.
- Preserve probability origin and derived-summary formulas.
- Isolate mutable KV/state caches across tenants/principals unless an explicit shared-cache policy is reviewed.

## Model/runtime interface

Conceptual API; exact repository API is chosen after B00/B01 intake:

```text
encode_state(StateInput, BackendFeatureManifest) -> StateHandle | EmulatedStateHandle
decide_batch(StateHandle, QuestionBatch) -> DecisionBatchResult
release_state(StateHandle)
```

An adapter may implement `encode_state + decide_batch` in one remote request. The semantic receipt remains the same so benchmarks can distinguish actual state reuse from API-shaped batching.

## Candidate-order contract

`CandidateManifest` records:

- candidate IDs and descriptions;
- generation/retrieval policy and version;
- canonical order rule and `order_digest`;
- any randomized/permutation seed;
- truncation/filtering/authorization outcomes.

Calibration is scoped to this construction policy. A changed ordering policy requires revalidation; it cannot silently reuse the previous calibration artifact.

## Question isolation conformance

For backends that claim isolated branches, run Q1 under:

- Q1 alone;
- Q1 + unrelated Q2;
- Q1 + contradictory Q2;
- Q1 + adversarial unused branch;
- Q1 + 100 irrelevant questions;
- reordered batch and equivalent cache state.

Use exact equality only for deterministic backends where that is promised. Otherwise preregister a statistical/tolerance contract and report drift rather than hiding it.

## Performance matrix

Measure separately:

1. state encoding/prefill;
2. incremental question cost;
3. incremental candidate/option cost;
4. branch scheduler overhead;
5. memory/KV residency;
6. cancellation waste;
7. end-to-end verified task cost/latency.

Compare serial calls, ordinary concurrent calls, shared-prefix/state reuse and specialized listwise models. 'One API request' is not reported as 'one forward pass' without backend evidence.

## Build sequence

1. Mock state handle and deterministic branch scheduler.
2. Emulated handle over existing generative adapter.
3. Direct-logit/sequence adapters with exact receipts.
4. Shared-prefix backend where supported.
5. Isolation/order/performance conformance suite.
6. AIR compiler scheduling integration.
7. Optional listwise learned model research only after the workload/bakeoff identifies value.

## Stop conditions

Stop/pivot if state reuse creates negligible end-to-end benefit, if question isolation cannot be preserved for a backend, or if memory/cancellation cost outweighs reduced prefill. Retain the stable ABI and fall back to serial/concurrent execution.

## v0.7 canonical decision encoding boundary

The Reflex Runtime owns one versioned canonical decision encoder used by model training exports, evaluation, live serving and semantic replay. Backend-specific tokenization may occur after this boundary, but any semantic transformation—state flattening, option description rewrite, omission/truncation, candidate reordering, delimiter policy—must be recorded in the effective-input receipt and calibration domain.

For packed isolated-question backends, branch masks and position policies are backend implementation details but are conformance-tested against separate inference. For cross-request reuse, `StateHandle` remains the public abstraction even when a backend implements only per-request packing and cannot persist KV/state across requests.

## v0.16 review — declared capabilities, not a universal state engine

`StateHandle` supports only operations declared by its backend profile. No cross-model KV interchange, latent compiler, automatic tensor serialization or bitwise cross-hardware replay is implied. Bind handles/cache entries to exact adapter/model/runtime/caller/epoch; current revocation and authority are checked even on a hit. A no-reuse adapter may preserve the logical interface but must report no reuse benefit. Generated probability vectors, direct logits, learned-head distributions and reliability estimates remain different semantic objects.

General Reflex service topology (the previously suggested CX-35) remains reserved work. Neural Program deployment contracts are defined by CX-23/CX-36; they do not fabricate a full generic serving ABI.

## v0.18 physical execution profile

Implement [P02–P08](../schemas/ACE_EXECUTION_PROFILE.md) as refinements of this runtime: independent deployment features, owner result families, complete scores, exact boundary scoring, branch visibility, exact effective input and truthful optional reuse. Use existing state/scheduler/host owners. A head-only backend need not expose KV; a generated probability does not become a native score.

## v0.19 runtime lowering

The Reflex Runtime exposes internal scheduling for `DECIDE`, dependency-compatible `FANOUT`, per-question `JOIN`, calibration lookup, risk gating and escalation. Shared-state/packed execution is an optimization beneath this semantic layer. The runtime records whether a backend actually shared state and what equivalence class was validated; it does not infer reuse from a fast response.
