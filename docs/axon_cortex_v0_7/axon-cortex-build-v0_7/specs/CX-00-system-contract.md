---
id: CX-00
title: "System contract and trust boundaries"
status: Draft
authority: Proposed
depends_on: []
first_stage: M0
implementation_evidence: []
---

# CX-00 — System contract and trust boundaries

## Intent and source basis

Specify what Cortex is allowed to promise and who may change those promises. Preserve Axon's interpreter-first and authority constraints, while refusing to infer stronger guarantees from phase-complete labels. Source basis: S1 pp.2–8, 12, 16, 23, 89–94; S2 pp.28–33 and 44–48. Sources resolve in [SOURCES](../SOURCES.md).

## Decisive fork

Choose a **hosted, interpreter-orchestrated cognitive runtime with a separately enforced action boundary**, not a new neural architecture or replacement kernel as the first deliverable. A hosted runtime is a first deployment tier, not abandonment of the separately documented bare-metal direction.

## Required contracts

Cortex comprises four logically separated domains: observations and artifacts; untrusted proposal/learning workers; trusted execution authority; and independently controlled verification/admission. A process boundary alone is insufficient if the worker can edit the other domain's policy or files.

| Requirement | Normative behavior |
|---|---|
| CX00-R1 | Every action MUST be authorized by the executor using a trusted grant, not a model answer, confidence, pathname, or claimed principal. |
| CX00-R2 | Task completion MUST be decided against a locked completion contract by a verifier the candidate cannot modify. |
| CX00-R3 | Every approval MUST name the exact artifact digest, policy version, environment constraints and authority scope. A subsequent mutation invalidates the approval. |
| CX00-R4 | A required gate that is absent, skipped, unsupported, expired or inconclusive MUST NOT be treated as passing. |
| CX00-R5 | Evidence MUST distinguish test observation, statistical estimate, formal proof, simulation and unknown. |
| CX00-R6 | Learned components MAY propose authority, gate or checker changes but MUST NOT activate them. Such changes require the existing Axon TCB/invariant governance path. |
| CX00-R7 | Runtime/model/schema/policy versions MUST stay pinned for an episode; a requested change starts an explicit new epoch. |
| CX00-R8 | Non-consequential development profiles MAY mark optional checks skipped; they MUST be visibly different and ineligible for consequential promotion. |

## Data and interfaces

Define a versioned `RunManifest` containing run ID, principal handle, goal/contract digests, initial snapshot, runtime/toolchain digests, model and tokenizer/adapter versions, policy/calibration IDs, sandbox profile, budgets, approved effects, evaluation cohort, and trace destination. A hash of secret material does not grant disclosure rights; secret references are handled by CX-10.

Define a `SafetyProfile` with mandatory gate IDs, permitted targets, effect budgets, grant issuer, verifier identity, admission identity, retention policy and whether external effects are permitted. Unconfigured consequential fields cause a refusal. Initial profile permits resettable local workspace edits and approved isolated tests only; no production deploy, network mutation, self-merge or TCB edits.

The candidate cannot obtain the verifier's signing credential or final evaluation corpus. A local development signer is explicitly marked non-production. The same model may supply suggestions to both domains, but a model opinion cannot replace the verifier's mechanical evidence.

## Failure and assurance semantics

Represent errors using symbolic Cortex categories plus phase, retryability, side-effect status and evidence references. Do not allocate new Axon numeric diagnostic/exit codes in this package. Map categories into existing namespaces only after the ledger is inspected. A process exit code alone is not authenticated evidence that a guard fired.

Guarantees are scoped: a capability checker constrains covered operations; native FFI and the host kernel remain trusted where documented. An independent checker can still be wrong; assurance requires explicit assumptions, adversarial tests and scoped proof, not the word independent alone.

## Acceptance gates

**G00-contract:** a fixture with a complete profile parses; each missing mandatory field refuses. A worker claiming another principal or fabricating a completion signature cannot execute/close.

**G00-mutation:** approve artifact A, replace one byte to produce B, and attempt execution. B is denied without a new approval. Policy-version mismatch also refuses.

**G00-missing-gate:** simulate PASS, FAIL, SKIPPED, UNKNOWN and unavailable mandatory verifier. Only actual passing evidence for all requirements permits admission.

**G00-authority:** a learner attempts to edit gate code, evaluation data, policy or signer credentials. The host denies access and records the denied attempt.

## Build slices and exclusions

First implement manifest/profile validation and a denial-only executor; then integrate approved local execution through CX-03/CX-13. Keep snapshots and evidence append-only by default. Do not add custom AI syntax, native cognition, a neural model, distributed quorum, or a kernel in this slice.

## Open decisions and evidence

Owner must approve the first host target, workload, budgets and admission authority. Evidence is currently empty; source claims are documented only. Completion requires recorded commands and artifacts for G00 plus a reviewed threat-boundary diagram in the real repository.

## v0.4 self-optimizing OS / external experience boundary

Cortex is the cognitive control plane of a self-optimizing Axon stack. Eligible experience may come from Axon's own execution, MiCode coding episodes, approved external repositories/histories, or generated curricula. Source does not imply trust. Every external artifact enters through schema validation, data-use policy and local Axon authority; no bridge can mint a principal grant or redefine required gates.
