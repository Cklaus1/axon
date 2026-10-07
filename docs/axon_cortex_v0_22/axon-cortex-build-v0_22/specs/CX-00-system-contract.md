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

## v0.16 review amendment — Trust profiles and default-deny adoption

Compilation, format validity, source binding, semantic validity, authorization and admission are separate properties. The runtime never upgrades one by inference. Candidate code, data, weights and validators run outside the protected authority/evaluator process unless an explicit independently approved profile includes their parser/backend in the TCB. Read-only state access can still disclose data or spend resources. Every deployment profile declares those effects separately.

**G00-assurance-separation:** fixtures with valid hashes/schema but false content, forged issuer, absent grant or stale admission cannot cross the corresponding stronger assurance boundary.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE ownership and claim scopes

ACE is an integration/physical-execution profile over existing owners. Adoption MUST reconcile live definitions and consumers, preserve reviewed CX-36 precedence, version document/fixture/runtime claims separately and keep automatic activation disabled. Crosswalk coverage is not product compliance.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G00-ace-owner-map:** Map each adopted contract to its existing owner, live definition, consumer and revision-bound gate; duplicate owners and definition-only completion fail.

**G00-ace-coverage:** All 86 source requirements retain anchored dispositions, owner/task/gate mappings and explicit supersessions/deferred scope; generated views and locks agree, with no product PASS from document tests.

## v0.19 amendment — calibrated decision claims remain non-authoritative

A calibrated decision can influence routing only within its declared event/domain scope. It cannot mint a capability, weaken an AcceptanceContract, mark a task complete, or replace a required independent verifier. The v0.19 decision-model slice is documentation/reference research until the real producer, calibrator, consumer and outcome join have product evidence.

**G00-v019-coverage:** The v0.19 package maps every new RLCD/Jev-derived requirement to an existing owner or an explicit research task, preserves CX-36 r0.2 and ACE v1 wire compatibility, and leaves every product result NOT_RUN.

## v0.20 amendment — schema decisions and lightweight Reflex specialization

The coordinated v0.20 / MiCode v0.14 amendment adds bounded schema-ingestion and lightweight fixed-task distillation profiles under existing owners. Schema compilation is not learned-artifact compilation; a learned Reflex may use unchanged CX-36 r0.2 while deterministic code remains a different representation. No new `.reflex` format, permission source, scheduler or admission authority is created. Package fixtures and successful source generation never establish product/model/interop PASS. Preserve prior live implementation evidence during import; do not reset a working repository to this document's NOT_RUN baseline.

**G00-v020-coverage:** All v0.20 schema/reflex requirements map to existing owners, tasks and falsification cases; generated views, frozen artifact schemas and MiCode v0.14 pins match; offline tests never mark live model/runtime/product gates PASS.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G00-r21-source-reconcile:** The upgrade records input archive hashes, targeted source evidence and live CX-35 ownership without renaming or deleting existing implementation or resetting repository completion evidence.

**G00-r21-owner-aliases:** EVO, DEC, EVL, RTR, CVM, SPX and TEL resolve to existing CX owners; no second promotion authority, capability registry, event store or Reflex wire is introduced.

**G00-r21-upgrade-preflight:** Preflight detects the v0.15 vendored path, legacy checksum schema, --report flag, zero-count rejection and report-hashing assumptions; it does not mutate the source, delete files, auto-stash or seal unknown changes.

**G00-r21-evidence-overlay:** Package validation receipts remain documentation-only; repository gate execution evidence is retained separately and no successful reference test is imported as a live gate PASS.

**G00-r21-v021-coverage:** The v0.21 manifests, requirement matrix, task DAG, gate declarations, alias owners and generated views are internally consistent, every gate is owned and no optional experiment enters a bounded nonresearch closure.

**G00-r21-v021-immutability:** Protected CX-36 r0.2 source/artifact/canonical contracts and existing ACE v1 JSON schemas remain byte-identical to the uploaded v0.20 pack; checksum inventories account for all deliverable files without self-hash cycles.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G00-r22-source-rebase:** The reviewed Axon/MiCode archives and target working trees are compared without writes; every changed, missing, unsafe and untracked path receives an owner disposition before editing. A matching filename or reported Git label is not a verified revision.

**G00-r22-preservation:** The v0.21 task/gate identities and meanings, CX-36 r0.2, ACE/neural wire formats, historical evidence and source-local identifiers survive migration; no live completion or runtime version is reset by importing this document pack.

**G00-r22-package-gate-upgrade:** The actual repository gate validates the 0.22 inventory/CLI/report semantics without zero-count false failures or self-hash cycles; an older vendored pack is retained until references migrate deliberately.

**G00-r22-pack-integrity:** Manifests, owner amendments, dependency closures, generated views, protected parent bytes and vendored Fabric bytes agree; every new task/gate has a source-derived or explicitly proposed rationale and execution recipe.

**G00-r22-honest-status:** All unexecuted product obligations remain NOT_RUN; offline reference/demo success is explicitly neither Rust implementation, physical backend evidence, real MiCode interoperability nor measured self-improvement.
