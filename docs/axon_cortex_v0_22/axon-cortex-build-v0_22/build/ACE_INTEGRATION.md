# ACE integration into existing Axon owners — v0.18

**Purpose:** implement one typed operation through existing Axon machinery, preserving its meaning while making physical execution observable and replaceable. This is a build guide, not a second scheduler/runtime or proof that live code already supports the profile.

Read [owner profile](../schemas/ACE_EXECUTION_PROFILE.md), [requirement crosswalk](../integration/ACE_REQUIREMENT_CROSSWALK.json), [decision ledger](../integration/ACE_DECISIONS.json), and [joint application instructions](APPLY_ACE_AND_MICODE.md). Existing CX-36 r0.2 formats remain frozen; no original ACE reference `.np` files enter the runtime.

## Implementation order

| Profile | Exit tasks | Scope / acceptance |
|---|---|---|
| AN0 `ace_contract` | B174, B175, B187 | Source/owner reconciliation, versioned projections/mappings, offline fixtures and package closure. Not live inference. |
| AN1 `ace_core` | B182 | One existing authorized real backend → one real Axon consuming call site; exact receipts, protected dispatch, refusal/cancel/fallback and recovery. No custom model required. |
| AN2 `ace_micode` | B183 | Same contract through real MiCode `/build-loop`/client, local permissions/check evidence and next-turn recovery. Depends on peer implementation, not peer documents alone. |
| AN3 `ace_three_family` | B184 | Optional hosted/open-scoring/real learned-head comparison on matched finite-choice tasks; missing arm blocks only this claim. |
| AN4 `ace_neural` | B185 | Separate real BuildLog → DiagnosticSummary Neural Program lifecycle; parser/SELECT-COPY baseline; admissible outcome includes rejection. |
| AN5 `ace_native_state` | B186 | Optional actual state reuse and multi-program isolation at the engine boundary, not merely fixture equality. |

Profile closures are computed from the task DAG. Optional ProgramAsWeights import, native-state translation, custom tokens, diffusion canvases and new foundation models are not AN1 prerequisites. Existing safe-repair and first-model gates remain applicable. First define owner contracts and passive receipts; do not wait for a mature learner or every backend.

## Reconnaissance and owner map

At the live repository revision, locate definitions **and consuming call sites** for AIR, host calls, interpreter/native execution, grants, provider adapters, candidate schemas, event store, effective-input materialization, registry/dispatch, verifier, model/worker lifetimes and existing state handles. Record current implementation and test evidence, unknowns, proposed change, migration and authority owner. The source package is not proof of these implementations. B174 resolves D01 for the scoped profile, not for all Axon.

Preserve existing task/spec IDs and statuses. B174–B187 are new bounded integration work; they extend older behavior without replacing B152–B173. Cross-references do not create task edges. If a live ID is occupied, allocate and retain a mapping rather than overwrite it. No CX-35 allocation or second type engine.

## First vertical slice

Use one owner-approved bounded decision (for example advisory test-family selection) from a real software observation. Freeze a small lawful fixture/task population and deterministic control. Materialize only authorized context. For exact deterministic tasks, use the exact baseline instead of adding inference solely to demonstrate it.

Run: approved intent → observation/working set → effective input → hard-filtered capability → typed request → real backend → validated decision/proposal → consuming host path → independent evidence. A suggestion to run a test passes the existing executor and grants; the model does not run it. Record selected/attempted/actual producer, failures, costs and result-family fidelity.

Required negative cases: unknown mapping, dynamic candidates sent to fixed head, missing complete scores, stale candidate/state, independent question consuming sibling answer, missing active result, unapproved fallback, budget exhaustion, revocation after selection, canceled late result, auth failure, and next normal call. Use mock replies for deterministic mechanics; real-model tests are separate authorized runs and cannot be replaced by those fixtures.

## Inner / outer / meta loops

**Inner:** choose one owner obligation; write a positive fixture and a distinction-losing negative test; implement at the existing definition and consumer; run focused tests; retain exact input, revision, output and limitation evidence. Update task/gate status only with subject-bound live evidence.

**Outer:** exercise the complete scoped host/client path. Test startup, use, failure, cancellation, retry/fallback, budget/egress, revoked handles and the following normal turn. Check that disabling the profile preserves incumbent behavior. Integrate only ready dependency slices; do not misreport optional missing arms as core failures or declare them completed.

**Meta:** inspect duplicate ownership, effects disguised as pure calls, schema-only or confidence-only acceptance, stale claims, hidden input/repair work, source downgrade and unsupported state/token promises. Preregister model/policy comparisons. Separate same-input deterministic parity from changed-model empirical evaluation. Narrow the profile instead of weakening a MUST. Architecture changes go through Intent/plan governance.

## Measurement

Report whole-operation outcomes and full attempt chains: valid/invalid outputs, exact/partial/missing scores, factual/source errors, abstention, task success, p50/p95 latency, prefix/suffix computation where measured, cold/hot startup, host/device memory, concurrency, paid tokens, retries/speculation/fallback, and unknown costs. Source claims of speed or accuracy are not acceptance thresholds. Real learned-head/native reuse claims require actual model/engine evidence on the qualified target, not a method name.

## Product closure

AN1 requires B182/G16-ace-core at a real consuming Axon call site. AN2 requires B183/G16-ace-peer with an exact peer contract lock and real MiCode records; offline round-trips do not satisfy it. Neural Program and three-family claims are independent. Reports may conclude reject/inconclusive, but must retain failures and prerequisites. This package performs documentation and inert conformance work only; all product gates remain NOT_RUN.
