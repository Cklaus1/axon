---
id: CX-05
title: "Axon Reflex typed inference and speculative questions"
status: Draft
authority: Proposed
depends_on: ["CX-01", "CX-04"]
first_stage: M2
implementation_evidence: []
---

# CX-05 — Axon Reflex typed inference and speculative questions

## Intent and source basis

Implement the fast bounded-judgment interface independently of any vendor or custom neural architecture. S2 pp.18–26 motivates operation selection plus branch-specific targets. W2 documents isolated questions against shared state; that is not a statement of statistical independence. See [SOURCES](../SOURCES.md).

## Decisive fork

Support several conforming backends and measure them on the same tasks and dynamic candidate manifests: (A) constrained-output generative adapter, (B) direct option-logit scorer, (C) sequence-probability scorer, and (D) learned option-conditioned/shared-encoder decision head. Keep generation and reasoning separate from Decide, and keep Decide separate from Act. Do not require a particular decoder or claim it reproduces an undisclosed vendor training method.

## Supported schemas and results

V0 offers BinaryDecision (with compatibility adapters free to map external Noul naming), Choice over a finite ordered runtime candidate set, and bounded ordinal Score with an explicit rubric. Rank, continuous prediction and open-ended extraction are extensions; arbitrary text/code belongs to Generate. Every question includes ID, definition, candidate IDs/descriptions, required dependencies, state digest, schema version and permitted abstention reasons.

A response reports selected candidate or abstention; optional genuine raw scores; optional complete derived probabilities; typed probability/uncertainty origin (`NativeOptionLogit`, `SequenceLikelihood`, `GeneratedEstimate`, `EntropyDerived`, `EnsembleEstimate`, `CalibratedEmpirical`, or `Unavailable`); inference/model/tokenizer/adapter versions; calibration reference or Uncalibrated; state/question/candidate-set digests; ordered candidate manifest; prefill/incremental timing where available; latency/usage/cost; and any refusal. A backend that only produces a label MUST NOT invent a probability. Model-reported confidence is marked self-report and ineligible for probability-based release routing without empirical calibration evidence.

Validate exact candidate membership; finite numeric values; range; normalization tolerance; field presence; branch tag; and snapshot identity. Invalid results remain failed observations, never silently coerced into successful decisions. The candidate ordering is part of the schema; test permutation sensitivity explicitly.

## Speculative conditional questions

For ActionKind, EditTarget, CheckTarget and InspectTarget, frame each target question conditionally: choose an edit target assuming the operation is Edit, for example. All questions refer to the same observation/candidate-manifest digest. After ActionKind resolves, validate and retain only its corresponding target output and any cross-field constraints.

Unused results do not become tool calls. They are still paid model work, and may still involve authorized network disclosure. Bound question count, total candidates, context bytes, KV-memory allocation and speculative spend. A dependent question that needs generated patch content or another answer's actual value must wait, or explicitly branch over a finite validated hypothesis set.

Parallel evaluation does not justify multiplying marginal probabilities to obtain joint correctness. Joint risk is calibrated for the complete selected action packet or treated conservatively. Contradictory independently produced outputs are rejected or escalated.

## Candidate-scoring details

Multi-token labels must be scored by a declared method, including complete sequences or a prefix-trie traversal; comparing only first-token logits is not a general solution. Register the normalization/length-bias treatment and any fixed opaque label encoding. Refit/recheck calibration when label construction changes.

Shared-prefix caching is an optimization with strict model/tokenizer/prompt/schema/tenant keys. Do not share mutable caches between tenants without isolation. Include long contexts, many candidates, shared token prefixes, Unicode, empty candidates and adversarial descriptions in conformance tests. No backend receives a blanket “one forward pass” or constant-time guarantee.

## Acceptance gates

**G05-schema:** all backend families conform to one dynamic-candidate ABI; malformed, missing, non-finite, outside-candidate and wrong-snapshot results fail predictably; label-only responses stay label-only; probability provenance cannot be forged.

**G05-branch:** a selected Check operation can only use CheckTarget; speculative EditTarget output never causes a write. Contradictory active fields cannot produce an action.

**G05-isolation:** correct state is compared with shuffled/empty/wrong/stale state, candidate reorder/rename, irrelevant context and adversarial options; state-insensitive shortcuts and sensitivity are reported; no false guarantee of independent errors is made.

**G05-tokenization:** multi-token candidates use a declared complete scoring method; exact opaque candidate IDs round-trip independently of human labels; candidate count/order/length sensitivity is measured.

**G05-performance:** compare generative/direct-logit/sequence/learned-head backends and serial/parallel/shared-state modes across context/cardinality/load sweeps. Publish prefill, incremental question/option, memory, end-to-end/backend cost/latency, selective decision quality and downstream task impact.

## Build slices and exclusions

Begin with mock and constrained generative adapters, then direct-logit and sequence scorers against one stable ABI. Build the grouped decision corpus and destructive controls before interpreting quality. A learned option-conditioned/shared-state neural head remains CX-14 research unless the B20 bakeoff justifies it. Provider success does not certify task completion, safety or calibration.

## v0.3 contract hardening

CX-05 additionally requires the executable conformance contract in [REFLEX_CONFORMANCE](../build/REFLEX_CONFORMANCE.md). Multi-question responses are keyed by `QuestionId` and may partially fail; the active branch defines which missing results block action construction. Choice, binary probability and ordinal score/distribution are distinct result families rather than a universal selected-label envelope.

Every backend publishes a `BackendFeatureManifest` and an `EffectiveInputReceipt`. Backend limits, silent preprocessing and truncation cannot be hidden behind a conforming response type. `ProviderReportedDistribution` is a valid raw evidence class distinct from native logits. Only the trusted adapter constructs privileged model instructions.

Candidate search must explicitly support `NoneSuitable`, `NeedMoreObservation`, `UnauthorizedCandidate`, and `InferenceUnavailable` states. Large candidate spaces are evaluated with registered retrieval/reranking or hierarchical/beam strategies; ranking heuristics such as path-score composition are not called calibrated correctness probabilities without separate evidence.

Question fusion/shared-prefix execution is an optimization only when the declared computation is preserved. Joining previously isolated prompts into one generative prompt is treated as a model/policy change and re-enters calibration/evaluation.

## v0.3 conformance acceptance gates formalized in the spec

**G05-batch-identity:** multi-question results are keyed by QuestionId; duplicate/unknown IDs fail; missing active-branch output blocks the action; unused speculative failure follows an explicit batch policy.

**G05-effective-input:** decisive evidence beyond a backend limit produces refusal or a visible authorized projection/truncation receipt; silent truncation fails.

**G05-prompt-boundary:** repository/log/replay text containing fake system/tool messages cannot become privileged adapter instructions or execution authority.

**G05-primitive-semantics:** Choice distributions, binary positive-event probabilities and ordinal distributions/expectations round-trip without silent rounding or invented confidence/logits.

**G05-candidate-absence:** no-suitable-option, need-more-observation, unauthorized-candidate and inference-unavailable paths remain distinct and produce the specified controller behavior.

## v0.6 shared-state Reflex runtime contract

Reflex is split conceptually into **Reflex Model** and **Reflex Runtime**. A conforming runtime MAY expose a two-stage optimization interface:

```text
encode_state(state, model_identity, preprocessing_policy) -> StateHandle
decide_batch(StateHandle, QuestionBatch) -> DecisionBatchResult
```

`StateHandle` is an optimization capability, not executable authority. It is immutable, bound to the exact state digest, model/tokenizer/adapter revision, preprocessing manifest, tenant/principal scope and expiry, and cannot be reused after any binding changes. Backends that cannot expose reusable state may emulate the interface in one call while still reporting equivalent receipts.

The runtime owns branch scheduling, candidate packing, response mapping, cancellation, budgets, cache lifetime/isolation and accounting. The model owns score/distribution production. This separation lets TypeSafe-compatible, direct-logit, sequence, learned-head and future Axon-native models compete without rewriting AIR.

Candidate-set inference is defined over the tuple `(state, question, candidate set, candidate order policy)`. Ordering is therefore part of the calibration/replay domain. Every dynamic candidate manifest records a deterministic construction policy and `order_digest`. High-assurance deployments MAY use registered permutation ensembles; the aggregation rule and extra compute must be explicit.

A decision distribution is primary. `top_choice`, entropy, margin, expected utility and any display `confidence_summary` are derived fields with formula/provenance. A derived confidence value is never silently promoted into empirical correctness probability.

### Question dependency classes

AIR/Reflex questions declare exactly one scheduling class:

- `Independent`: answer does not depend on sibling answers; eligible for the same shared-state batch.
- `ConditionallyRelevant(branch)`: framed under an explicit branch assumption and eligible for speculative evaluation; only the selected branch may be consumed.
- `AnswerDependent(question_id)`: requires an earlier realized answer and must execute in a later stage unless the graph explicitly expands a finite validated hypothesis set.

Batching/fusion is semantics-preserving only when these dependencies and prompt-role boundaries remain unchanged. Joining previously isolated questions into one conversational/generative prompt is a model-policy change, not a compiler optimization.

### Additional acceptance gates

**G05-state-handle:** state encoding is computed/reused only under matching state/model/tokenizer/preprocessing/tenant bindings; a stale or cross-tenant `StateHandle` refuses. Backends without reusable state still emit equivalent effective-input and timing receipts.

**G05-question-isolation:** for backends claiming isolated question semantics, Q1 alone and Q1 batched with unrelated, contradictory, adversarial and 100 irrelevant sibling questions stay within a preregistered tolerance under the backend's pinned deterministic/stochastic evaluation protocol. Failure is reported as loss of the isolation claim, not normalized away.

**G05-order-domain:** candidate permutation tests are part of conformance and calibration. Order-sensitive backends record the exact order policy/digest; a calibration artifact cannot be reused after candidate construction/order policy changes without revalidation.

## v0.7 control candidates and canonical encoding

Finite-option inference must represent the possibility that the current candidate compiler is incomplete or that escalation is required. Where a decision family permits it, the candidate compiler may append typed control candidates such as `NONE`, `OBSERVE_MORE`, `ESCALATE` or `BLOCKED`. These are ordinary typed candidates with policy-defined semantics; they do not grant authority. A backend may never synthesize an unregistered control candidate.

The canonical decision encoder/renderer is a versioned contract shared by training, evaluation, serving and replay. It owns reserved-token escaping, state/question/candidate serialization, branch boundaries, position policy and candidate-ID mapping. Training against one encoding while serving another is distribution shift and invalidates calibration until revalidated.

Candidate-order robustness is desirable but does not erase order from semantics or provenance. Every answer remains bound to the exact candidate manifest and `order_digest` that was evaluated.


## v0.10 probability-source refinement

Reflex results must preserve how probability-like values were obtained. At minimum distinguish model-generated estimates, selected-token logits, decision-head/pointer distributions and empirically calibrated outputs. A wire-compatible System-One response does not imply equivalent mathematical semantics. CX-24 owns the shared provenance vocabulary for semantic perception/matching as well.
