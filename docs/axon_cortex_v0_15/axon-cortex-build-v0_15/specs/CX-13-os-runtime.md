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
