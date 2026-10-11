# ACE — Axon Cognitive Execution profile — `axon.ace-execution/1`

**Document status: proposed Axon-owned integration contract, introduced in build package v0.18.** No network endpoint, binary ABI, production model, native cache, or live interoperability is asserted. This profile refines existing owners; it does not create an ACE runtime. The normative obligations below govern adoption of this profile. JSON examples and the reference validator implement a deliberately limited, inert projection of it.

## P01 — Authority, scope and versioning

ACE means **Axon Cognitive Execution**. It is an integration and physical-execution profile over AIR, Reflex, Neural Programs, scheduler, state and evidence owners. The common abstraction is a **typed cognitive operation**, not a Neural Program. Neural Programs remain one learned-artifact capability class.

The supplied ACE v0.2 references older Cortex v0.15, MiCode v0.8 and the original CX-36. Reviewed Axon v0.16/CX-36 r0.2 is the baseline. The [source lock](../integration/ACE_SOURCE_LOCK.json), [86-requirement crosswalk](../integration/ACE_REQUIREMENT_CROSSWALK.json), and [decision ledger](../integration/ACE_DECISIONS.json) retain the original claims and identify amendments. Where the older source conflicts, the recorded resolution applies; nothing retroactively authorizes live work.

Separate document version (`0.18`), this logical integration profile, underlying owner schema versions, provider protocol versions, and native engine/ABI versions. The [JSON schema](json/ace-execution.schema.json) is `axon.ace-projection/1`: a portable record projection, not a replacement for all owner protocols. Any native/wire implementation MUST supply a versioned lossless mapping for the subset it advertises. Unknown mandatory variants and semantic losses refuse. Historical lossy views must identify losses and remain ineligible for protected use/training unless separately adjudicated.

CX-35 stays reserved. Neither a general serving project nor a model-state compiler is synthesized here. Embedded, local-sidecar and remote modes may reuse existing approved backend/host interfaces, with the same semantic obligations and independently negotiated support.

## P02 — Four axes, one mapping owner

Keep `air_node_kind`, MiCode `cognitive_class`, `operation_kind`, and registry `capability_class` distinct. Also retain `physical_mechanism` and `deployment_mode`. A model token, AIR opcode, candidate ID, source object ID and artifact digest are different namespaces. Short IDs do not supply unseen object contents.

CX-04 owns AIR semantics; CX-15 owns host/interpreter/native lowering. CX-34 selects capabilities. The [mapping table](../integration/ACE_VOCABULARY_MAP.json) freezes only these initial rows:

| Mapping | AIR | MiCode accounting | Operation | Capability | Physical path |
|---|---|---|---|---|---|
| `reflex.general` | Decide | REFLEX | DECIDE | GENERAL_REFLEX | structured generation, token/sequence scoring or learned head |
| `reflex.specialized` | Decide | REFLEX | DECIDE | SPECIALIZED_REFLEX | token/sequence scoring or learned head |
| `generation.proposal` | Generate | GENERATE | TRANSFORM | GENERATE | structured generation or validated text |
| `neural.transform` | Generate | GENERATE | TRANSFORM | NEURAL_PROGRAM | learned function with CX-36 artifact and proposal result |
| `rule.projection` | Rule | RULE | PROJECT | RULE | registered deterministic projection |

`neural.transform` is an effectful **host-call mapping**, not a new syntax or a claim that every Neural Program is a generator. A Neural Program that implements a bounded decision needs its own reviewed mapping and complete Reflex conformance; that extension is not implied by this initial subset. Other valid existing AIR workflows remain supported by their existing contracts, but MUST NOT be forced into these rows. Missing mappings return Unsupported for this profile.

Every invocation binds input/output types, contract, capability/artifact identity, intent, dependencies, visibility and AI/network/compute/resource effects. Rule cannot hide inference. New syntax/native lowering requires CX-15's type-map audit, concrete workflows, diagnostics, desugaring and parity or refusal. Recorded-reply interpreter parity verifies deterministic host integration only; changing model, prompt, context, candidate construction or fusion requires empirical evaluation/admission, not a compiler-equivalence assertion.

## P03 — Feature negotiation, not an all-or-nothing tier

CX-05's BackendFeatureManifest and CX-34's registry entry expose **independent** deployment-scoped features: result families, modalities, dynamic candidates, complete scores, selection policy, question isolation, batching, native-state operations, cancellation, usage, limits and immutable identity. A head-only backend is valid; raw KV/hidden-state access is not required.

Each feature is Supported, Unsupported, Unknown or Emulated. Supported/Emulated claims carry scoped evidence and exact backend/mode identity; emulation additionally names its mechanism, costs and evaluated semantic mapping. A client cannot require a feature and silently accept Unknown. Emulation satisfies a requirement only through an explicitly allowed, evaluated mapping; otherwise refuse. A configured feature flag is not conformance evidence.

Limits bind context, candidate cardinality, question count, result size/depth, queue/concurrency, memory, retries and root budget. Check before dispatch. Runtime changes invalidate the feature snapshot unless a registered compatibility policy covers them. Health/readiness, loaded assets, feature conformance, applicability, and admission are separate facts. Immutable identity unavailable from a hosted provider stays unavailable; profiles requiring it cannot claim support.

## P04 — Reflex results and selection

CX-05/CX-06 retain distinct **Choice**, **BinaryProbability**, and **OrdinalDistribution** projections. The source owner's `BinaryDecision` and ordinal Score names map only when their semantics support the target family. A Boolean-only answer cannot satisfy a probability request. An expectation-only score cannot satisfy an ordinal-distribution request. Neural Program invocation results stay in CX-36's `ProposedValue`/absence/failure union, not in a generic Choice envelope.

Requests bind QuestionId, request/operation, observation, working set, effective input, definition, candidate construction/authorization/order policies and exact candidate manifests/digests. Candidates are unique IDs with supplied meaning and typed values or authorized materialized projections. Control candidates appear only when the owner policy declares them; they are not fabricated by a model.

A complete distribution covers exactly the request's IDs once, finite in-range values and a registered normalization tolerance. Partial top-k is not complete; do not fill missing values, add a uniform distribution or normalize away absent mass. Normalization, aliasing and repair are versioned transformations linked to original scores and charged attempts. A label-only Choice is allowed only when explicitly requested and has no invented probability.

Choice selection policies are separate from available scores: provider choice, required argmax, declared sampling, or distribution-only. Argmax requires complete genuine scores and a declared stable tie rule. Sampling retains policy/randomness and reproducibility limits. BinaryProbability names the positive event. OrdinalDistribution retains ordered rubric/level identity; an expected value is a declared derived view, not a replacement for the distribution or an assertion that an ordinal scale has equal intervals.

Raw source names are retained alongside explicit canonical mappings. Canonical CX-05 origins include ProviderReportedDistribution, NativeOptionLogit, LearnedDecisionHead, SequenceLikelihood, GeneratedEstimate, EnsembleEstimate and Unavailable. For the prior MiCode fixture only, TokenLikelihood maps to NativeOptionLogit **only** with candidate-token mechanism evidence; DecisionHeadDistribution maps to LearnedDecisionHead only with learned-head evidence. The original producer vocabulary/tag and mapping revision remain in the receipt. Unknown tags are not guessed. `CalibratedEmpirical` is not a replacement raw origin.

Distribution, entropy, margin, correctness estimate, applicability estimate and OOD score are not interchangeable. Every correctness estimate names its exact event, estimator, fitted/calibration/validation data and applicability domain. Missing is Unavailable/Uncalibrated; outside domain is Inapplicable. Definition, input encoder, candidate count/order/construction, model, normalization or deployment changes invalidate calibration unless covered by evidence. No generic confidence threshold grants permission or proves correctness.

## P05 — Exact scoring and learned-head conformance

For one-token scoring, the backend MUST validate distinct labels at the actual tokenizer/chat-template/assistant boundary, including whitespace, special tokens and prefix stability. Preserve prompt-visible aliases, candidate order and token-map digest. Conditional token scores are preferences among those labels, not empirical P(correct). The runtime may expose zero generated output tokens while still performing substantial prefill/head compute; state what was measured.

Multi-token scoring uses the complete sequence and declared termination convention. First-token shortcuts cannot claim whole-answer probability. Length normalization, constrained generation and trie/grammar selection are distinct computations requiring their own profiles. None may silently substitute for another.

A native learned-head profile binds base/model, tokenizer/processor, feature layer/pooling/position, tensor shape/layout/dtype, quantization, adapter, renderer/encoder, label order or dynamic-conditioning recipe, output semantics and learned-parameter digest. Fixed K heads cannot claim arbitrary dynamic candidate support. Fixture logits, lookup-by-test-ID and text generation behind a `head()` method are not evidence of native execution. Actual conformance needs real learned parameters on qualified unseen inputs. No specific architecture/vendor is mandatory.

## P06 — Working sets and native state

CX-02 observations, CX-28 WorkingSetReceipt, exact EffectiveInputReceipt and CX-05 runtime StateHandle are different objects. A ContextSnapshot is only a view over them. Model inputs must actually materialize authorized referenced facts or validated features; opaque references alone do not communicate those facts. Preserve protected pins, omissions, compression, redaction, roles, template, candidates and committed parent inputs. A receipt may bind access-controlled material rather than duplicate sensitive data in telemetry.

Native state binds backend instance/epoch, model/base/adapter/tokenizer/renderer/processor, relevant position/attention/precision/cache policy, observation/working-set/effective-input, principal/project, policy generation, lease/expiry and current authorization. Mismatched bindings invalidate reuse unless a separately evaluated compatibility profile allows it. Durable records contain stable receipts, not raw pointers. Unsupported operations remain Unsupported; no implicit KV concatenation or cross-model/cross-engine transfer.

Distinguish actual reusable state, opportunistic provider-prefix reuse, rematerialization/recorded replay, none, and unknown. A cache hit, one HTTP call, resident base or batch is not proof of one neural pass, no input transmission, zero billing, zero decoding or constant memory. Handle release is idempotent logical lifecycle management, not proof of remote byte erasure. CX-13 owns memory quotas, cleanup and process boundaries.

Async selection/results whose intent/state/catalog/lease changed are excluded from live commit. Independent branch state cannot mutate another branch or its shared prefix. Context eviction uses CX-28's RecomputeContract; historical/effectful/mutable data is not silently regenerated. Remote rematerialization is a new authorized attempt, not a free cache operation.

## P07 — Dependency-safe batching and composition

Preserve Independent, ConditionallyRelevant and AnswerDependent semantics. MiCode lower-case spellings map explicitly through the vocabulary table. Independent branches do not consume sibling answers; they may share declared context. A backend whose prompt reveals other questions' definitions MUST advertise that visibility; isolation of answers is not isolation of all question text. Do not claim question noninterference without tests for the advertised visibility profile.

AnswerDependent work waits for committed parent results and binds their identities. A separately declared bounded hypothesis expansion is not the same computation as consuming an actual parent answer. ConditionallyRelevant work may run speculatively, but only the selected active branch may commit.

Join by QuestionId, never array position. Reject duplicate/unknown IDs, stale bindings and dependency cycles. Require all active-branch values and cross-field invariants; unused speculative failures are tolerated only by a versioned policy. Record and charge unused work even when discarded. Partial diagnostic output is not a committed artifact. Marginal probabilities are not a joint-success probability.

Combining isolated questions into a joint conversational prompt, changing visibility or statistically fusing answers is a policy/model variant, requiring conformance, calibration and task evaluation. Recorded replies cannot establish fresh-inference equivalence.

## P08 — Dispatch, attempts, cancellation and fallbacks

CX-34 performs hard type/authority/privacy/project/applicability/evidence/admission/hardware filtering before utility ranking and rechecks current grants/revocation at dispatch. Bind release, target, policy generation, input projection and reservation at the dispatch boundary. Stable tie rules belong to the reference scheduler; learned routing carries its admitted policy/reproducibility limits.

Every physical inference/reasoning/repair/retry/fallback attempt is a separate receipt under one root operation, deadline and resource budget. Selected and actual implementations are recorded separately. Downstream effectful tool actions have their own identities; an inference attempt is not a shell/test execution. Include unsuccessful, speculative and fallback work in denominators and costs. Unknown usage is not zero. To claim a hard monetary cap for a provider with unknown costs, enforce a conservative reservation/upper bound or refuse that profile.

At most one terminal outcome per attempt enters committed state. Distinguish value/proposal, abstention, refusal, failure, unsupported, deadline, local cancellation, confirmed remote cancellation and OutcomeUnknown. Receiving an inference value does not make a task VerifiedComplete. A late result after terminal cancellation/deadline remains uncommitted. Later reconciliation of unknown remote work is new evidence, not rewriting the old receipt or resuming canceled actions.

Root cancellation propagates to children; where the backend cannot prove remote stop, preserve that uncertainty and the possible cost. Durable compilation reconciles job/idempotency/provider identity after timeout rather than blindly resubmitting. Idempotency strings alone do not prove exactly-once execution.

Fallback is an explicitly allowed bounded acyclic graph of new invocations with remaining root budget/deadline and unchanged-or-narrower hard constraints. Permission denial, invalid artifacts, missing credentials and offline requirements are not reasons to evade controls with another provider. A failure may have an observed producer but no accepted producer; record both accurately. Inference, artifact/model download, training/compilation, publication and activation are separate privileges.

## P09 — Neural Programs preserve reviewed CX-36

CX-36 r0.2, `axon.nps/1`, `axon.np/1` and `axon.cjson/1` remain unchanged by this integration. Their schemas and inert parser fixture are hash-locked. The original ACE `ace-reference-zip-0.2`, self-excluding manifest identity and inline evaluation/admission examples are historical reference only, not valid current artifacts.

CX-23 remains compiler/runtime lifecycle owner, with separate compiler and inference interfaces/credentials. CX-34 owns the class-specific registry view; CX-11 owns detached release/admission. A frozen executable artifact precedes evaluation; evaluation, reliability and admission attach through detached release records. Compilation cannot activate. Candidate/shadow/canary/active labels are not grants. Normal dispatch requires current eligibility; explicit experiments may load authorized candidates.

Both `.nps`-driven and eligible-experience-driven compilation preserve source/contract/recipe/data partitions/label provenance/job/artifact lineage. Ordinary prompts and grammar templates remain Generate/Reflex assets; only genuinely learned executable assets with CX-36 evidence qualify as Neural Programs. Remote ProgramAsWeights compilation/import is optional, never a core dependency.

`ProposedValue` structural, source-bound and semantic validation remain separate. Copying a source span does not prove semantic correctness; valid output cannot certify permissions or completion. Applicability is checked separately from loading and calibration. Missing specialization never silently runs a bare base while reporting the artifact ID. Shared-base claims require at least two programs, exact identity, isolation and bounded memory tests. Offline readiness requires the complete validated dependency closure and real network-disabled testing.

The learned-function pilot stays **BuildLog → DiagnosticSummary**, with immutable source-span checks, exact parser/SELECT-COPY and incumbent baselines. Preserve files, locations, codes and causal errors without inventing facts, fixes or tool calls. A real lifecycle ending in rejection is useful evidence. A three-label classifier or an inert `.np` fixture is not this pilot.

## P10 — Evidence, experiments and self-application

Use existing CX-32/MX-30 graph and source journals; do not build a new ACE evidence store. Link intent/source → working set/effective input → registry/selection/attempt → typed result → action/world change → check/evidence → admission/rollback. Preserve contradictions, failed attempts, invalidations and missingness. Do not upgrade predicted/counterfactual results into observations.

Record receipt replay, fresh inference, resettable environment rerun and counterfactual simulation as different modes. Tests on recorded replies prove only their declared host semantics. Benchmark complete task quality, abstention/coverage, source factuality, transfer, p50/p95 latency, cold/hot setup, memory, input/output/reasoning/speculation, retry/fallback and compile/maintenance costs. Preregister populations, family splits, controls, thresholds, stopping and evaluator feedback budgets; no universal speedup/quality number is invented here.

Instrumentation starts with the first useful integrated call; autonomous promotion does not. CX-20/CX-21/CX-33 evaluation and CX-11 admission remain outside ordinary self-improvement. Preserve data rights, sensitive/tainted content boundaries, corpus roles, retention and transfer scope. Visibility of evidence is not training/export authority. Routine receipts do not expose credentials or private reasoning. Negative transfer may narrow or quarantine capability scope.

## P11 — MiCode exchange and operator truth

CX-16 exports this profile to MiCode's existing MX-05/12/17/18/22/26/30/32/33 owners. MiCode remains owner of `/build-loop`, credentials, tool registry/PermissionGate, local workspace/effect execution and AcceptanceContract completion. An Axon admission or model judgment is not local permission.

Every build run binds approved IntentIR, BuildSpec meaning/document digests, DAG revision and installed build-loop identity. Required semantic/authority/acceptance changes raise IntentConflict or PlanChangeProposal. No new executor is introduced and an unavailable installed skill is not claimed present.

Operator interfaces distinguish requested → selected → attempted → observed producer → accepted producer, including fallback, refusal, missing evidence and unknown cost. Auth-recovery links come from trusted configured provider metadata. Canceling/failing an attempt cannot leave a hidden model selected while displaying a different one on the next turn. A real consuming call site and next-turn recovery test are mandatory for an integrated claim.

T0–T5 coding-frontier tiers and P0–P5 cross-project tiers retain their namespaces. Shared artifacts require receiving-project re-admission; bridge mappings cannot broaden authority or collapse these tiers.

## P12 — Security and optional research

Untrusted repositories, logs, package descriptions, labels and model replies remain data even when they contain system/tool delimiters. Trusted adapters alone construct privileged instructions. Bound parser/request/result/artifact sizes and depth, queue/concurrency, device/host memory, root attempts, traces and all retries. Authenticate principal/project/session, not caller-supplied scope strings or hashes. No package installation hooks execute during inspection; no training service hides inside inference.

Local-only applies to tokenization, telemetry, redirects, model/adapter/dependency fetching, inference, fallback and compilation—not merely the main hostname. A local sidecar/worktree is not a sandbox. Use existing CX-03/CX-13 policy and tested host guarantees; fail the affected profile when those prerequisites are absent.

Dedicated token vocabularies, learned embedding/KV generation, cross-engine/model state translation, diffusion/infill canvases, world-model transition ABIs and compiler/native promotion remain separately admitted research. Each needs its own pinned representation, semantics, units/horizon, lifetime/privacy/limits, conformance and empirical evidence. Unsupported research cannot be silently emulated with prose or become a core dependency.

## P13 — Verification boundary

All new product gates remain NOT_RUN. A source crosswalk, valid JSON, format fixture, hash, alias, support flag or receipt is not authenticated execution, calibrated quality, protected verification or admission. Gate closure requires the actual subject revision, definition **and consumer**, authorized environment and named live tests. Any symbol anchor mechanism must survive line shifts and reject a changed symbol body. This profile's offline validator tests structural and selected semantic invariants only.


## P14 — ACE v1 execution-fabric completion

CX-37 supplies the provider/runtime ABI, immutable `ACEExecutionPlan`, general `ACECognitiveResult` union, streaming/finalization, hard cognition/effect boundary, provider `DataHandlingProfile`, isolation lifecycle, determinism class, durable `ACEExecutionHandle` and semantic-preserving fallback rules. These extend this profile without moving scheduler, registry, effect execution, admission, evidence or Neural Program ownership.

The legacy name **ANEA (Axon Neural Execution Architecture)** refers only to the v0.1/v0.2 historical source bundle. New APIs, schemas, tasks, gates and consumer exports use ACE. See `integration/ACE_LEGACY_ALIAS_MAP.json`.
