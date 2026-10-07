---
id: CX-13
title: "Hosted OS integration, resource control and confinement"
status: Draft
authority: Proposed
depends_on: ["CX-03", "CX-04", "CX-10"]
first_stage: M1
implementation_evidence: []
---

# CX-13 — Hosted OS integration, resource control and confinement

## Intent and source basis

Provide the operating-system responsibilities needed for safe long-running cognition: process authority, isolation, scheduling, persistence, cancellation and recovery. S1 pp.16, 62–63 and 84–88 documents hosted services and unresolved kernel/VM scope. See [SOURCES](../SOURCES.md).

## Decisive fork

Adopt staged host tiers. Cortex v0 depends on a tested hosted executor, not an Axon-authored kernel. Native test/build artifacts still run in external confinement; interpreter capability checks do not automatically restrict arbitrary native subprocesses.

## Tiered roadmap

Tier H0: one supported host, single-writer isolated workspaces, no production effects, supervisor-managed model/tool jobs. Tier H1: durable multi-job service, quotas, revocation, observability, recovery and separately approved remote adapters. Tier H2: parity/equivalent-contract host portability, optional microVM and hardware-attestation integration. Tier K: separately approved bare-metal branch with syscall/confinement evidence. Tier K is not an implied deliverable of H0–H2.

Support status is per host/profile and version. A missing sandbox feature refuses real tool execution; a documented mock can run non-consequential fixtures but never pass a production-confinement gate.

## Supervisor and job contract

The trusted parent owns grants, credentials, policy, immutable artifact store, required-gate execution and job state. Untrusted model/generated-code workers receive only a projected input, scoped handles and an ephemeral filesystem view. The parent must not load arbitrary worker code into its own address space.

`JobManifest` binds executable/artifact, principal, input snapshot, working directory, resource reservations, permitted mounts, secret/environment projection, egress policy, deadline, cancellation channel and expected output schema. Restrict descendants as well as the initial process. A sandbox that limits the command string but lets descendants access the host is insufficient.

Reserve aggregate CPU/memory/GPU/time/token/cost/storage budgets where supported. Report estimated versus actual model usage and reconcile with available provider receipts; an estimate alone cannot guarantee exact monetary spending. Hard local deadlines and conservative call/output limits remain enforcement tools. Backpressure bounds queued work and abandoned speculative inference.

## Confinement and secrets

No worker sees the parent's verifier/admission credentials. The environment is an allowlist projection, not inherited ambient state. Mount only permitted content, make protected artifacts read-only, and prevent escape through symlinks, replacement executables, inherited descriptors and shared caches.

Network-denied workloads must be denied at the actual host boundary. For later scoped egress, validate the chosen enforcement mechanism against names, resolved destinations, redirects, proxy behavior and credential forwarding. Do not infer effective network controls from an annotation or an untested container flag.

Source/build dependencies are code. Dependency fetching and builds need separate policies, pinning and isolated artifacts. The initial release uses approved offline fixtures; live package installation is not silently authorized by RunBuild.

## Cancellation and recovery

Cancel propagates from trusted supervisor to all child processes/model requests where possible, prevents new dispatch, records possibly ongoing remote work, and never clears a latched stop without the approved lifecycle. Worker acknowledgement alone is not evidence that it stopped. Test actual termination and resource release under the configured bound.

Restart reconstructs durable jobs/actions and reconciles OutcomeUnknown; it does not replay effects as a recovery strategy. A worker cannot modify the kill latch, supervisor binary or root policy. S1 distinguishes supervisor kill behavior from interpreter flags; preserve that distinction.

## Attestation and portability

Claims identify whether evidence is simulated, local hash/HMAC, or hardware-rooted, and exactly which loaded components/artifacts are covered. Do not call a digest proof that the runtime remained uncompromised or that a decision was correct. Reuse R26–R34 only after their target-specific gates are reproduced.

For each added engine/host, run a contract matrix: isolation, effects, cancellation, replay, resource limits and evidence. Equivalent user-visible guarantees matter more than identical host implementation. Unsupported cases refuse explicitly.

## Acceptance gates

**G13-escape:** adversarial build/test fixtures try host reads/writes, unapproved subprocesses, network access, environment secrets and protected evaluator paths. Allowed operations succeed; denied operations produce no unauthorized realized effect.

**G13-kill:** stuck process trees and slow inference are canceled under the declared bound; no further dispatch or resumable self-clear occurs.

**G13-quota:** concurrent child jobs cannot oversubscribe carved quotas or evade them through restart.

**G13-recovery:** parent/worker crashes at action boundaries produce safe reconciliation and no blind external retry.

**G13-tier:** unsupported/native profiles cannot inherit interpreter-only guarantees; simulated attestation cannot satisfy a hardware-required profile.

## Build slices and exclusions

Implement H0 before untrusted test execution in M1, then operational H1. Desktop UI, drivers, general kernel replacement, multi-VM consensus and hosted commercial service packaging are separate decisions.

## v0.3 external-service adoption

Protected hosted backends must satisfy the dependency/deployment contract in [DEPENDENCY_ADOPTION](../build/DEPENDENCY_ADOPTION.md). Example unauthenticated endpoints or permissive demo configurations are research-only until principal authentication, transport/network policy, tenant/cache isolation, budgets, observability and revocation are established.

## v0.3 external-adoption gate formalized in the spec

**G13-adoption:** a protected external backend has pinned source/transitive model/tokenizer/encoder identity, reviewed license/security profile and visible SDK transformations/retries; unauthenticated demo deployment fails protected admission.

## v0.16 review amendment — Shadow resource and disclosure isolation

Shadow workers cannot affect task control or production state, but still need explicit grants for compute, storage, audit writes and any model-service disclosure. Use bounded separate quotas, cancellation and foreground priority. Embedded inference does not provide a sandbox; untrusted model/container parsers cannot run inside the grant signer or verifier process by default.

**G13-shadow-egress:** a no-task-effect shadow request to a disallowed provider, unapproved model download or exhausted shadow resource pool refuses before disclosure/dispatch; foreground work cannot be starved by unbounded shadow fan-out.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE amendment — ACE physical attempt and resource lifecycle

P06/P08/P12 apply to all claimed host/deployment modes. Reserve root budgets across branches, retries, repairs and fallbacks; distinguish local cancellation, confirmed remote stop and unknown remote work. Exactly-once actions or remote erasure are not inferred from handles/idempotency. Enforce actual locality and queue/device/host bounds.

See the normative [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md) and [build guide](../build/ACE_INTEGRATION.md). These requirements extend this owner; no second ACE subsystem is introduced.

**G13-ace-attempts:** Exactly one terminal result per attempt may commit; canceled/deadline late replies never apply, unknown remote work remains explicit, and retries/fallbacks share enforced root budgets without treating unknown cost as zero.

**G13-ace-locality:** Offline/local-only profiles deny hidden tokenization, telemetry, redirects, downloads, fallback and compilation egress; principal/session/lease and bounds are enforced by actual host controls, not localhost names.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G13-r21-budget-authority:** Research telemetry does not create a second spend authority; existing principal reservations bound all branches and unknown charges, and actual reconciliation neither double-charges nor releases unresolved liabilities.

**G13-r21-economic-controls:** Whole-task comparisons include uncached/cached input, output, full verifier/fallback costs, latency and failure coverage under fixed price/model/environment revisions; warm-only measurements are not presented as total production savings.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G13-r22-profile-eligibility:** Only the exact independently qualified profile/configuration/host within its freshness policy can satisfy requested isolation; source_inspected, experimental, withdrawn or unresolved capability combinations are refused before effects.

**G13-r22-legacy-scope:** Legacy interpreter/program runs retain declared-effect and authority semantics and existing parity tests; a fake Axon filename cannot dispatch arbitrary native shell work.

**G13-r22-no-weak-fallback:** A request requiring qualified microVM protection is refused or paused when unavailable; rollback, timeout, feature disable and provider selection never route it to an ordinary subprocess or Git worktree.

**G13-r22-journal-before-effect:** A crash at each reservation/journal/dispatch boundary recovers one recorded operation without blind re-execution; same OperationId with changed immutable request is refused, and the tested persistent store survives process restart.

**G13-r22-aggregate-reservation:** Sibling jobs, model calls, verifier work and retries share an atomic task/experiment ceiling; concurrent reservations cannot overspend and failed or cancelled work is not dropped.

**G13-r22-unknown-reconcile:** A timeout after a possible effect yields OutcomeUnknown with outstanding cleanup/billing liability; no exactly-once claim, immediate free-budget refund or automatic duplicate-effect retry is inferred from an idempotency key.

**G13-r22-vm-cli-parity:** The existing axon-vm CLI and applicable legacy tests retain behavior after extraction; provider SDKs and native codegen are not introduced into core interpreter/browser builds.

**G13-r22-guest-truth:** The custom Axon guest demo is never advertised as Linux or a complete native execution environment; backend labels enumerate tested engine/enclosure/guest/OS/architecture combinations rather than a tier hierarchy.

**G13-r22-profile-qualification:** Profile qualification binds source/build, engine/image/configuration, tested host and nonzero product assertions to a trusted evidence issuer; fixture data or a verified flag without those receipts cannot enable protected dispatch.

**G13-r22-launch-cleanup:** Injected failures after process/cgroup/socket/disk acquisition leave no unowned resources; cancel acknowledgement is not cleanup completion and stopped is not destroyed.

**G13-r22-billing-settlement:** Duplicate accounting receipts are idempotent only under identical origin/sequence/content; unresolved usage remains unknown with conservatively reserved liability and cannot contribute a spurious zero-cost winner.

**G13-r22-rollback-lifecycle:** Crash during activation or rollback reconciles the durable journal and active pointer without split-brain authority; active jobs are cancelled/drained according to policy and liabilities remain accounted.

**G13-r22-restart-matrix:** Actual process/store restarts at every important effect boundary leave no unowned worker or silent repeated consequential operation; cleanup and financial obligations reconcile or remain explicitly pending.
