---
id: CX-37
title: "ACE Provider Runtime, Execution Plan, and Cognitive Result ABI"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-03", "CX-04", "CX-05", "CX-06", "CX-10", "CX-11", "CX-13", "CX-23", "CX-28", "CX-32", "CX-34", "CX-36"]
first_stage: M2
implementation_evidence: []
---

# CX-37 — ACE Provider Runtime, Execution Plan, and Cognitive Result ABI

## 1. Purpose

ACE — **Axon Cognitive Execution** — is the governed bridge from a typed cognitive operation to the physical implementation that actually performs it. AIR defines cognitive semantics; the Cognitive Scheduler chooses among eligible capabilities; ACE binds the exact plan, revalidates it at dispatch, invokes a provider, and returns a typed result plus truthful execution receipts.

This spec turns the execution profile into an implementable fabric without creating a second scheduler, registry, verifier, Neural Program runtime, effect executor, or world model.

```text
Intent / AIR
    ↓
Cognitive Scheduler + Capability Registry
    ↓
ACEExecutionPlan
    ↓ dispatch-time revalidation
ACEProvider
    ↓
ACECognitiveResult / ACEStream
    ↓
validation + evidence
    ↓
internal cognitive state OR ProposedAction
    ↓
CX-03 capability/effect executor for world effects
```

## 2. Ownership and non-goals

- CX-04 owns AIR semantics.
- CX-34 owns capability registry and scheduling policy.
- CX-03 owns application-world effects and authority enforcement.
- CX-05/CX-06 own Reflex semantics and calibration.
- CX-23/CX-36 own Neural Program lifecycle and artifact contracts.
- CX-32 owns evidence graph lineage.
- CX-11 owns admission.
- CX-37 owns the provider/runtime contract, execution-plan binding, general cognitive-result union, streaming/finalization, provider data-handling declaration, reproducibility class and durable execution handles.

ACE does not expose or require private chain-of-thought. Evidence and decision provenance are sufficient.

## 3. ACEExecutionPlan

The scheduler MUST materialize a versioned immutable plan before physical dispatch:

```text
ACEExecutionPlan {
  plan_id
  operation_contract_digest
  air_operation_ref
  intent_ref
  authority_snapshot_ref
  capability_id + revision
  provider_id + backend revision
  required_features
  input_projection_digest
  result_semantics
  state_handle_ref?
  data_handling_requirements
  determinism_requirement
  budget_reservation
  deadline
  fallback_graph
  policy_generation
}
```

The plan is evidence, not authority by itself. Dispatch rechecks current capability/admission/revocation, principal/project/session, policy generation, privacy constraints, budget, provider readiness and state-handle validity. A stale plan refuses or is rescheduled explicitly; it is never silently rebound to a different provider.

## 4. ACE Provider Runtime ABI

A provider implementation MUST map a declared subset of the following logical ABI without semantic loss:

```text
ACEProvider {
  describe() -> ProviderDescriptor
  prepare(plan) -> PreparedExecution | Unsupported
  execute(prepared) -> ACEExecutionHandle
  stream(handle) -> ACEStreamEvent*
  cancel(handle) -> CancelReceipt
  reconcile(handle) -> ExecutionStatus
  release_state(state_handle) -> ReleaseReceipt
  health() -> ProviderHealth
}
```

Embedded traits, local IPC and remote transports MAY differ physically. The advertised logical semantics, identities and result families MUST remain stable. Unknown mandatory variants refuse. Transport adapters cannot invent stronger provider guarantees than the provider exposes.

## 5. General cognitive result union

ACE MUST distinguish result kinds at the type level:

```text
ACECognitiveResult<T> =
    DecisionDistribution<T>
  | ProposedValue<T>
  | Prediction<T>
  | EvidenceResult<T>
  | GeneratedArtifact<T>
  | ProposedAction<T>
  | Abstention
  | Refusal
  | Unsupported
  | Failure
  | OutcomeUnknown
```

`Prediction` is not `Observation`. `GeneratedArtifact` is not a verified artifact. `ProposedAction` is not an executed action. A result is accepted only under the consumer contract that requested its exact semantic family.

## 6. Streaming and partial execution

Streaming providers emit ordered `ACEStreamEvent`s with `execution_id` and monotonic `sequence_no`. Events may contain tentative chunks, progress/usage, warnings and terminal finalization. Tentative chunks MUST NOT become committed cognitive state, evidence of completion, or effect authorization merely because they parse successfully.

The final event binds the final digest, result family, validation outcome, terminal status and complete known usage. Interrupted streams retain partial cost and an explicit terminal/unknown status. Reconnect/resume requires provider support and handle identity; clients cannot reconstruct continuity from text alone. Backpressure and bounded buffering are implementation requirements.

## 7. Hard cognition/effect boundary

ACE providers perform cognition. They MUST NOT directly perform application-world effects such as file edits, shell commands, deployments, database writes, messages or financial actions under hidden provider tooling.

A provider that supports native tool calls MUST either disable them or surface each request as a typed `ProposedAction` for normal CX-03 capability/effect authorization and execution. Provider-internal retrieval or compute is permitted only when declared by the DataHandlingProfile and effect/resource contract.

## 8. Provider DataHandlingProfile

Every provider/deployment identity MUST declare a versioned data-handling profile covering at least:

```text
retention_policy
provider_logging
training_use
region_or_residency
subprocessors
transport_encryption
server_side_cache
telemetry
artifact/model downloads
requested deletion semantics
```

`remote_allowed` is insufficient. The scheduler hard-filters providers against the operation's data policy before utility ranking. Unknown mandatory privacy fields are not treated as permissive defaults.

## 9. State isolation and lifecycle

Cross-principal and cross-project reuse of KV, hidden state, semantic caches, prompt-prefix state or mutable adapter-local state is forbidden by default. State handles bind provider/backend/model/tokenizer/renderer/position/principal/project/session and lease policy.

The provider declares partition, lease, eviction and release behavior. Logical release and proven physical erasure are separate claims. Cache hits and reused state remain billable/resource-accounted according to actual provider semantics.

## 10. Determinism and reproducibility

Every attempt declares one `DeterminismClass`:

```text
DETERMINISTIC
SEEDED_REPRODUCIBLE
BEST_EFFORT_REPRODUCIBLE
NONDETERMINISTIC
UNKNOWN
```

Where relevant, receipts bind seed, sampling parameters, precision, quantization, runtime/kernel revision and hardware class. Replay claims MUST state whether they are exact, numerically tolerant, statistical, recorded-only or unavailable.

## 11. Fallback semantic preservation

A fallback provider must satisfy the original requested result semantics and all hard constraints. A generated probability estimate cannot satisfy a request requiring native option logits; an expectation-only ordinal result cannot satisfy a full distribution; a text-model guess cannot masquerade as a world-model prediction.

If the caller explicitly permits alternate semantic classes, each fallback is a new typed attempt and the accepted result records the changed class. Otherwise return `Unsupported` or the relevant failure state.

## 12. Durable ACEExecutionHandle

Long-running or remote work uses a durable execution handle:

```text
ACEExecutionHandle {
  execution_id
  provider_operation_id?
  idempotency_scope
  submitted_at
  deadline
  principal/project/session
  plan_digest
}
```

Submit, poll/attach, cancel, reconcile and finalize semantics are standardized. A timeout does not prove remote cancellation. Retrying without reconciliation may create duplicate work and is forbidden unless the provider's idempotency contract proves the retry safe.

## 13. Security and transport

Provider descriptors, health data, model catalogs and capability claims are untrusted until authenticated according to deployment policy. Bounded sizes, deadlines, concurrency and resource reservations apply before deserialization or model invocation where practical. Credentials never enter model-visible state unless the operation contract explicitly requires a secret-bearing provider primitive and policy allows it.

ACE's transport is not a sandbox. Local sidecar and embedded deployment inherit host authority unless constrained by existing Axon sandbox/capability mechanisms.

## 14. Evidence and self-improvement

CX-32 records plan → revalidation → provider descriptor → attempts/stream/final result → downstream validation/effects. Model/provider selection and result acceptance remain independently inspectable. Self-improvement may optimize provider selection, plan construction or streaming policy but cannot publish providers, weaken hard filters, rewrite evidence or change admission without normal protected gates.

## 15. MiCode contract

MiCode consumes ACE as a cognitive provider contract while retaining local ownership of credentials, PermissionGate/tool execution, `/build-loop`, AcceptanceContract and completion verification. Imported ACE results remain evidence/proposals until MiCode's owning local checks accept them.

## 16. Acceptance gates

**G37-naming:** Current Axon architecture, protocols, tasks, gates and consumer exports use ACE/Axon Cognitive Execution; ANEA remains only a historical source alias with an explicit compatibility map.

**G37-execution-plan:** An immutable ACEExecutionPlan binds operation, capability/provider revisions, exact input/result semantics, authority/privacy/budget/deadline/fallback and policy generation; dispatch detects stale/revoked/rebound plans.

**G37-provider-abi:** Embedded, sidecar and remote providers implement the same advertised logical describe/prepare/execute/stream/cancel/reconcile/release/health semantics for their supported subset, with explicit Unsupported/Unknown behavior.

**G37-result-union:** Decision, proposal, prediction, evidence, generated artifact and proposed action remain distinct; prediction/generated/proposed values cannot become observations, verified artifacts or executed effects by coercion.

**G37-streaming:** Tentative stream chunks cannot commit state/evidence/effects; ordering, backpressure, interruption, usage and finalization are bounded and tested, including disconnect after valid partial output.

**G37-effect-boundary:** Provider-native tool/action requests cannot bypass CX-03; hidden application-world effects are rejected or reified as ProposedAction and separately authorized.

**G37-data-handling:** Provider retention/logging/training/residency/subprocessor/cache/telemetry/download declarations are versioned and hard-filtered against operation policy; unknown mandatory fields fail closed.

**G37-isolation:** Cross-principal/project native-state reuse is denied by default; handle binding, lease, eviction and release semantics prevent stale/cross-tenant/cache contamination without claiming physical erasure absent evidence.

**G37-determinism:** Every attempt records a determinism class and applicable seed/sampling/precision/runtime/hardware identity; replay claims use the matching exact/numerical/statistical/recorded/unavailable class.

**G37-durable-handle:** Timeout/cancel/retry/reconcile over durable handles preserves unknown remote work and prevents blind duplicate submission; terminal result identity remains unique and auditable.

**G37-fallback-semantics:** Fallback never silently weakens requested result semantics, privacy, authority, deadline or budget; allowed semantic changes are explicit new typed attempts.

**G37-transport-conformance:** A reference in-process provider and one transport adapter round-trip the same bounded fixtures without semantic loss, invented guarantees or hidden execution; product interoperability remains NOT_RUN until exercised on the real implementation.

## v0.19 amendment — calibrated correctness claim semantics

ACE v1 wire schemas remain unchanged. `DecisionResult.distribution` / binary `probability` describe the producer's decision event. An empirical probability that the selected decision is correct uses the existing `correctness` object and is `Estimated` only when `domain_ref`, `estimator_ref`, `event_ref` and `evidence_ref` resolve to a CX-06-valid calibration artifact/key for this producer, effective input and candidate policy. Otherwise correctness is `Unavailable`/`Inapplicable` even if a raw distribution exists.

Providers MAY implement shared-state/packed execution, but the ABI exposes only truthful feature/equivalence claims; implementation topology is not inferred from latency. Provider-native confidence or vendor-calibration claims are not silently upgraded into Axon calibration evidence.

**G37-calibrated-claim:** ACE accepts `correctness.status=Estimated` only with resolvable matching calibration domain/estimator/event/evidence provenance; raw distributions, vendor confidence or stale calibration cannot be retyped as empirical correctness without that evidence.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

Schema-generated complete invocations remain proposals through existing result unions and local owners; no new ACE wire tag or source syntax is introduced. A calibrated fixed-task head requires the exact event/domain/task/label/runtime qualification described in the [profiles](../schemas/LIGHTWEIGHT_REFLEX_PROFILE.md). Unknown features, OOD, failed active outputs or missing calibration route to an explicitly allowed fallback/refusal without broader privacy/authority. Bounded action IDs avoid some generated text but do not replace argument resolution, execution or independent verification.

**G37-schema-proposal-only:** Schema/reflex outputs map to existing proposal/decision semantics; no new wire tag, high score, compact action ID or composition completeness bypasses permission, finalization or independent completion.
