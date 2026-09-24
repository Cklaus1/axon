---
id: ACF-01
title: Axon Compute Fabric
version: 0.1.0
status: Draft
authority: Proposed
source_snapshot: 4cceb89 (reported by uploaded context bundle)
source_date: 2026-09-24
implementation_evidence: []
product_gates_executed: 0
---

# Axon Compute Fabric

## 1. Decision and boundary

**Extend Axon's existing authority and execution seams into a capability-negotiated compute service. Do not replace its interpreter, supervisor, Cortex runner, host interface, or Firecracker launcher with a parallel sandbox framework.**

ASI OS may expose a user-facing `Computer` resource. Axon owns admission, identities, workload state, budgets, execution evidence, and promotion. A native or remote backend supplies execution resources, not authority. Cortex chooses semantic actions and evaluates outcomes; the fabric places and manages the execution those authorized actions require.

This document is a new, independently versioned ACF addendum. It is not a claim that Cortex v0.20 was reviewed or modified. The archive contains the vendored Cortex v0.15 package and an evolving source implementation; its context README reports commit `4cceb89`. A target checkout must be re-inventoried before applying this proposal. The package uses ACF identifiers rather than claiming an unverified CX or R-series allocation. [E01, E33–E36]

`MUST`, `MUST NOT`, and `SHOULD` below describe proposed acceptance requirements. They do not describe features already implemented. Source evidence identifiers resolve in `review/SOURCE_EVIDENCE.md` and `manifests/evidence_index.json`.

### 1.1 Correcting the earlier conceptual model

The inspected source does not establish three production-ready peers named WASM, microVM, and full VM.

* `axon-os::Runtime` already abstracts declared effects, principal minting and bounded execution. Its `AxonCoreRuntime` runs the canonical interpreter in a subprocess with an Axon wrapper. It is not a general operating-system sandbox for arbitrary native descendants. [E03–E06]
* `axon-vm` is a **binary-only Firecracker microVM launcher**. In this tree, “VM” in this crate's name is not a separate general-VM product. There is no snapshot/pause/fork command in its inspected API. [E11–E18]
* `axon-wasm` is an interpreter compiled as a browser-oriented `wasm32-unknown-unknown` library. A generic server-side WASM hosting service with independently verified fuel, memory and host-call confinement is additional work. [E22–E25]
* `axon-guest-kernel` is a separate research guest. Its K5 code explicitly leaves interpreter ELF loading and a VFS unfinished; the current branch demonstrates a syscall-denial path. It MUST NOT advertise a working Linux computer, shell, general interpreter workload, or desktop. [E21]

Consequently, the first release is an **integration and confinement milestone**, not a universal forkable-computer release.

## 2. Goals, release boundary, and non-goals

The first protected vertical slice MUST execute one registered Cortex check in an isolated Linux workspace, under the existing authority boundary, with bounded resources, an independently attributable result, and no write to the operator's source workspace. A second request MUST NOT cross-contaminate its inputs, IDs, credentials, handles or budget.

The initial release contains contracts, a truthful backend registry, a native Linux execution route, immutable workspace materialization, reservations, cancellation/reconciliation, and Cortex evidence linkage. A legacy subprocess route remains available for its existing scoped/interpreter use and trusted fixtures; it is never silently selected to satisfy a hardware-isolation or arbitrary-native-code requirement.

Logical workspace branches follow that vertical slice. Full RAM checkpoints, memory forks, a generic bounded WASM host, one remote provider, desktop takeover, GPU placement and learned routing are separately gated extensions. They are not prerequisites for the first useful release.

Non-goals are a new language VM, a rewrite of `axon-core`, a replacement for Cortex's cognitive scheduler, a second identity/approval system, universal live-memory migration, automatic merge of machine state, automatic production deployment, eleven vendor integrations, and completing the custom guest kernel as a dependency of hosted execution.

## 3. Ownership and integration

### 3.1 Reuse existing owners

| Concern | Existing owner / source | Proposed integration |
|---|---|---|
| Language execution semantics | `axon-core` interpreter; `AGENTS.md` | Preserve interpreter-first behavior and parity gates. |
| Per-builtin host effects | `axon-core/src/host.rs::AxonHost` | Reuse for Axon host I/O, recording and virtual hosts; it is not the machine lifecycle API. |
| Grant algebra, approval and containment admission | `axon-os::{grant,approval,gate,supervisor}` | Keep as authority owner; introduce a shared compute-admission path without bypassing the existing one. |
| Legacy program execution | `axon-os/src/runtime.rs::Runtime` | Add an opt-in `FabricRuntime` bridge for compatible `.axjob` program runs. |
| Firecracker execution | `axon-vm/src/main.rs` | Extract a library with CLI compatibility, then add an explicitly profiled backend. |
| Workspace observations and semantic action authorization | `axon-cortex::{WorkspaceSnapshot,runner::Authorized}` | Preserve these types and digests; link durable workspace artifacts instead of renaming observations into disk snapshots. |
| Registered test execution | `Runner::run_tests_json` | Inject `CheckExecutor`, with a fabric-backed implementation and a fixture implementation. |
| Episode evidence | `axon-cortex::episode`, `axon-os::RunRecord` | Add versioned references to execution receipts; preserve existing encodings. |
| Cognitive selection | Cortex/CX-34 | Supply feasible execution profiles and measurements; do not select goals or grant authority. |

These ownership choices follow the source seams and the existing CX-03/CX-13 contracts. [E03–E10, E24–E36]

### 3.2 Dependency direction

A proposed `axon-compute` library MAY hold pure protocol types, capability/profile matching, workspace manifests, durable operation state and the backend contract. It MUST NOT depend on `axon-cortex`, `axon-core` with native codegen, `axon-os`, or a provider SDK. A module-first implementation is acceptable if it preserves this dependency direction; the source's CX-04 guidance explicitly warns against premature crate splits. [E35]

`axon-os` owns the admitted service and optional `FabricRuntime`; it consumes these contracts. An extracted `axon-vm` library implements the native microVM backend and remains dependent on `axon-attest` as appropriate. A small integration module in a trusted host binary wires authority to drivers. Driver SDKs MUST be feature-gated and absent from core interpreter/browser builds. Avoid a cycle in which `axon-os` depends on a fabric that depends back on `axon-os`.

`axon-cortex` gets a narrow `CheckExecutor` seam, not direct vendor credentials or API dependencies. `cortex-policy-adapter` remains a typed boundary and cannot turn possession of a JSON compute request into permission to execute it.

The existing `axon-vm` CLI may remain directly usable by an authorized host operator. Inside a worker it MUST be inaccessible as a bypass to the fabric. Feature disabling changes routes, not the protection an admitted workload requires.

### 3.3 Conceptual flow

```text
ASI OS Computer / Cortex registered action
                    |
             axon-os authority
       approval + scoped grant + current epoch
                    |
        admitted ComputeJob / reservation
                    |
         Axon Compute Fabric broker
          /          |           \
 legacy .ax      Linux microVM    later bounded WASM / remote
 interpreter     axon-vm library  adapter
          \          |           /
           evidence + reconciled usage
                    |
      independent verifier / Cortex episode
                    |
      explicit workspace promotion decision
```

## 4. Engine, isolation, placement and capabilities

Do not encode `WASM < microVM < VM < remote` as a hierarchy. A remote machine can run WASM; a local microVM can run a native process. A hardware boundary and a language effect checker protect different things.

Every supported **backend profile** binds one concrete combination:

`engine + enclosure + guest_kind + OS + architecture + backend_version + configuration_digest + placement_policy`.

Possible engine labels include `axon_interpreter`, `axon_wasm` and `native_process`. Enclosures include `process_scoped`, `bounded_wasm_host`, `hardened_linux` and `kvm_microvm`. Guest kinds distinguish `none`, `linux_init` and `axon_kernel_demo`. `local` and `remote` are placement, not security grades.

A profile MUST enumerate the exact combinations it implements. Independent sets such as “supports Windows” and “supports memory snapshots” MUST NOT imply that their combination works. GPU/desktop/nested-virtualization flags likewise apply to a profile, not the company name.

Separate three categories:

1. **Functionality:** can the profile execute a registered check, stream a terminal or capture memory?
2. **Enforcement:** can it enforce this mount scope, default-deny egress, child-process limit and cancellation bound?
3. **Authority:** may this principal perform this operation on this resource now?

A registry entry is not a grant. Support evidence is versioned as `not_implemented`, `unknown`, `experimental`, `verified`, or `withdrawn`, with a reason and the tested host/profile/configuration. A `verified` claim requires nonempty product-test evidence, freshness policy and an appropriate evidence issuer. Schema-valid or vendor-advertised does not mean verified.

Routing first filters for authority, data residency, isolation, exact requirements, quotas and healthy evidence. Only then may it minimize expected completed-task cost or latency. No backend satisfying all hard requirements produces `Unsupported` or an explicit authorized escalation; it never produces an implicit weaker fallback. Data export is a separately authorized effect.

## 5. Canonical resource model

### 5.1 Identity is not content identity

Use separate identities for `Task`, `ExperimentArm`, `Trial`, `Attempt`, `Operation`, `Execution`, `Branch`, `WorkspaceVersion`, `Checkpoint` and `Artifact`. A retry of a transport operation reuses its `OperationId`; an authorized new execution attempt gets a new `AttemptId`. Repeated trials of the same task and arm have distinct `TrialId`s.

A task-definition digest and deterministic seed describe reproducibility; they do not identify a unique execution. This is important because the present Cortex episode name is derived from target path, symbol and hidden check and repeats for repeated runs of that same request. Keep that useful semantic identifier and add execution IDs around it. [E30]

A runtime handle binds tenant, principal scope, execution ID, backend profile, provider resource, lease ID and fencing epoch. It is an opaque service reference. Knowing a handle string does not authorize attach, read, resume or destroy. A stale handle cannot operate on a recycled resource.

### 5.2 Resource definitions

| Resource | Required meaning |
|---|---|
| `EnvironmentSpec` | Content-bound executable/interpreter, dependency closure, base image, guest kind, OS/architecture, approved configuration and startup recipe. |
| `WorkspaceVersion` | Immutable retrievable contents and metadata, parent references, tenant/label, exact scope and completeness declaration. |
| `WorkspaceLease` | A bounded mutable worktree derived from a version, with one writer and fenced promotion rights. |
| `SemanticStateRef` | Reference to the owning agent/Cortex store; the fabric does not invent or overwrite agent memory. |
| `ExecutionCheckpoint` | Typed reference to captured local execution state, capture scope, consistency level, compatibility envelope, provenance and retention. |
| `ComputeJob` | A validated request binding all authority, input, executable, resource and output requirements for one execution attempt. |
| `RegisteredExecutable` | Trusted mapping from check/tool ID to executable digest, argv schema, environment projection, effect ceiling, output contract and required enclosure. |
| `ExecutionReceipt` | Observed lifecycle outcome, exit data, verifier status, usage and evidence origins, without conflating them. |
| `ComputeEvent` | Ordered, versioned evidence with task/trial/attempt/operation and causal references. |

`WorkspaceSnapshot` in Cortex remains a scoped observation of hashes. The new `WorkspaceVersion` is not a retroactive reinterpretation of it. An explicit projection links the observation digest to the durable object and lists omissions. [E27–E28]

## 6. Admission and API contract

### 6.1 Preserve existing paths

The current supervisor performs approval, declared-effect admission, grant intersection, mint, execute and record. Keep that convergence point; a new command, replay path, GUI attach or remote driver cannot skip equivalent checks. [E04]

The existing `Runtime` signature is program-oriented and synchronous. `FabricRuntime` MUST implement only the `.axjob` operations that retain those semantics. It MUST NOT interpret a fake `.ax` filename as an arbitrary shell request or convert unknown native effects into an empty effect set. The current flat manifest parser is not extended silently. [E03, E08]

Introduce an explicit, versioned `ComputeJob` request for registered checks and later native tools. The trusted registry supplies the executable, approved argv shape and declared maximum effects. The caller supplies only validated arguments and references. Arbitrary generated native content remains untrusted even under a registered build command. Authorization covers the dependency closure and the actual executable artifact, not a display name or entry file alone.

Extract shared admission logic where required rather than maintain two independently evolving policy implementations. Existing `Grant::intersect` remains the basis for filesystem, network and executable ceilings; new rights such as checkpoint, export and attach live in a supervisor-owned registry extension. They MUST be included in the new authority digest and revocation logic.

### 6.2 Request validation

A request binds principal/session, task/trial/attempt IDs, input version, executable and dependency digests, registry entry version, working directory, permitted mounts, environment projection, network policy, result schema, reservation, deadline, cancellation scope and idempotency semantics. A reference is resolved from trusted state before use.

The effective authorization is the intersection of job request, supervisor authority, inherited scoped authority, tenant policy and current resource/export restrictions. Backend capabilities only narrow eligible implementation; they never expand authority.

Admission MUST fail closed on unknown fields in a closed schema, duplicate JSON keys including escaped aliases, malformed digests, absent required facts, expired approval, revoked epoch, unsupported policy translation, missing executable binding, and absent resource reservations. Closed enums never default to the closest known value. [E26, E33–E34]

### 6.3 Host-side API shape

The following is a proposed contract, not compiled source or an existing public API:

```text
ComputeService.submit(actor, untrusted_request) -> OperationRef
ComputeService.inspect(actor, execution_ref) -> Observed<ExecutionStatus>
ComputeService.cancel(actor, execution_ref, reason) -> OperationRef
ComputeService.reconcile(actor, operation_ref) -> ReconciliationResult
ComputeService.checkpoint(actor, execution_ref, capture_request) -> OperationRef
ComputeService.fork(actor, checkpoint_or_workspace_ref, branch_plan) -> OperationRef
ComputeService.attach(actor, execution_ref, attach_scope) -> ScopedSession
ComputeService.destroy(actor, execution_ref, retention_policy) -> OperationRef
```

Every method rechecks authorization. A bare backend `exec` API MUST NOT be exposed to model output. Inside the trusted host, drivers receive an immutable launch plan after admission. That plan is not a reusable or user-mintable bearer credential. The host binds and checks operation IDs and current fencing epochs immediately before side effects.

An internal backend contract needs `describe_profiles`, `preflight`, `start`, `inspect`, `cancel`, `collect_receipt`, `reconcile` and `destroy`. Checkpoint, restore, fork and attach are negotiated extensions, not methods that silently succeed with empty results. Drivers return structured `Unsupported`, `Denied`, `Failed`, `OutcomeUnknown` or typed success. A transport timeout is not a successful cancellation.

### 6.4 Approval binding

A new approval receipt binds the operation class, principal, executable closure, workspace version, policy/configuration digest, maximum resources, destination/placement and expiry. Changing any security-relevant binding requires readmission. Existing approval formats are preserved; they cannot be upgraded into stronger assurance merely by placing them inside a new envelope.

A hash binds bytes; it does not authenticate an issuer. Cross-host approval needs an authenticated authority channel or verified signature and trusted issuer lookup. Existing local hashes or software attestation cannot satisfy a requirement for hardware-rooted confidential execution. [E26, E33]

## 7. Durable lifecycle, cleanup, cancellation and retry

### 7.1 Two distinct state machines

Keep action state separate from machine state. The CX-03 action sequence remains:

`Proposed → Prepared → Validated → Running → Observed → Verified`.

Its failure alternatives remain Refused, Failed, Canceled and OutcomeUnknown. `Verified` is a verifier conclusion, not a process exit. [E34]

A machine resource uses:

`Requested → Admitted → Provisioning → Ready → Running → Stopping → Stopped → Destroying → Destroyed`.

Negotiated branches include `Running → Quiescing → Checkpointing → Running|Suspended`, `Suspended → Restoring → Ready`, and `Unknown → Reconciling → observed state`. A failed checkpoint must report whether the original is still usable. `Failed` does not imply resource deletion.

### 7.2 Journal before effects

Before provisioning, executing, forking or promotion, atomically persist the operation identity, authorized input/configuration digest, resource reservation, expected prior version and intended transition. Persist observations and provider receipts afterward. Crash recovery reconciles outstanding operations; it does not invoke `axon-os replay` as an effect-recovery mechanism. That existing command intentionally re-executes a job. [E10, E33–E34]

For the first single-host implementation, a durable transactional journal with uniqueness constraints, compare-and-swap state updates and an outbox is sufficient. The chosen store and its crash semantics must be documented and fault-tested. An in-memory map or a stream of stdout lines is not the journal. Existing audit records provide integrity evidence; they do not by themselves prove atomic reservation, locking, persistence or issuer authentication.

A duplicate request with the same operation ID and same immutable inputs returns its recorded operation. The same ID with different inputs is rejected. A new trial is not deduplicated just because its program and seed match an older trial.

### 7.3 Failure certainty

Each effect adapter declares whether it supports server-side deduplication, safely repeatable reads, or reconciliation-required execution. A timeout after a possible effect produces `OutcomeUnknown`. The service MUST NOT promise exactly-once external side effects merely because it stores an idempotency key.

Automatic provider fallback is allowed before a consequential effect begins. After a possible effect, reconcile or require an approved new action. Billing uncertainty remains reserved liability until settled or explicitly written off by trusted policy.

### 7.4 Resource ownership

A launch guard owns every child process, cgroup, socket, mount, temporary file, network namespace, relay task and provider handle from first creation. All failure paths clean up, and cleanup failure remains a visible reconciliation obligation. Never rely solely on the success-path code at the end of a function. The existing launcher has fallible operations after spawn and cleanup at its normal end; this must be hardened before pooling. [E14–E17]

Replace per-process temporary names with per-attempt private directories, restrictive permissions and safe atomic file creation. Separate operation identity from process ID. Concurrent executions in one supervisor must not share the wrapper path or vsock/API socket namespace. [E05, E15]

Cancellation first latches against new dispatch and revokes relevant credentials/leases, then terminates execution and descendants, then verifies stop and reconciles usage. A process-group signal alone cannot be the claimed confinement guarantee for arbitrary descendants. The supported Linux profile must demonstrate termination of deliberately detached descendants and resource release. Remote stop uncertainty is reported, not hidden.

A suspended or restored worker cannot clear the supervisor's cancellation latch. Out-of-band authority and fencing epochs remain current even when guest memory is old.

## 8. Workspace and state architecture

### 8.1 Four layers, with bounded portability

| Layer | Owner | Contract |
|---|---|---|
| S0 Environment | Axon artifact/registry services | Reconstructible definition for explicitly supported OS, architecture, dependency and license constraints. |
| S1 Workspace | Axon workspace store | Durable contents, history, labels and approved exports. |
| S2 Execution state | Backend-specific capture | Optional accelerator with an exact compatibility and durability envelope. |
| S3 Semantic state | Cortex/agent memory owner | Versioned reference in execution lineage, not a copy of all memory into the VM. |

Logical migration is supported **only when** the destination can satisfy the environment, authorization and export requirements. It is not guaranteed across incompatible operating systems, architectures, missing dependencies, unsupported secret policies or unexportable vendor storage. A RAM capture is never the only copy of an accepted artifact.

### 8.2 Durable workspace content

Store actual immutable bytes, not only file hashes. The manifest includes normalized relative path, entry type, content digest, size, required mode/executable bit, permitted link metadata, parent relation, label and capture scope. Empty directories, missing paths, file deletion and unavailable observation are distinct states.

Capture includes explicitly selected dirty and untracked inputs; it does not assume Git tracked files equal the environment. The first release may deny symlinks, hard links, devices and unusual metadata rather than implement them unsafely. Denial is explicit. Later support must defend resolution/replacement races at open time, not just normalize a string once. Protected verifier files, credentials and host files are never part of a writable worker projection.

Materialization must verify each object, enforce size/count/path-depth bounds, create a private worktree and make required protected inputs read-only. Archive import is untrusted: reject absolute paths, traversal, link escapes, device entries and decompression/resource amplification. No provider-local path is accepted as a portable artifact reference without import and verification.

The existing `Runner::stage_copy` copies only immediate regular files. Keep it a fixture helper until a separate recursively safe materializer is tested; do not advertise it as workspace backup. [E32]

### 8.3 Promotion

Workers produce immutable candidate versions. An independent verifier runs against the precise candidate plus a protected verifier/environment digest. Promotion checks that the base reference and authority epoch still match, then atomically advances the approved workspace reference with a compare-and-swap. A stale base produces a conflict and explicit rebase/reverification, not an overwrite.

First release: one writer per branch and promotion authority separate from execution authority. Parallel branches have independent writable trees. Shared mutable volumes are not permitted as a shortcut. No automatic RAM merge exists. Branch selection does not replay writes to external services or deploy a candidate.

## 9. Checkpoints, resume and forks

### 9.1 Explicit checkpoint kind and completeness

Use distinct kinds: `logical_workspace`, `filesystem`, and `machine_state`. Do not call a file copy a memory checkpoint. Each artifact declares exactly what was captured, omitted, quiesced and durably stored, plus whether consistency is application-consistent or only crash-consistent.

A `machine_state` compatibility envelope binds backend/VMM version, configuration, CPU architecture/features, guest kernel, device topology, disk/image/volume versions, memory format, network/vsock restrictions and any relevant accelerator limitations. The adapter MUST reject incompatible restore. GPU memory and external databases are omitted unless independently supported and tested.

A snapshot is immutable content. Suspend is a lifecycle transition. Restore creates or resumes an execution under a new admitted operation. Fork creates independent descendants. These are separate permissions and APIs.

### 9.2 Capture protocol

Admission → reserve storage/quota → fence new effects → quiesce application/workspace or declare crash consistency → capture supported state → verify object integrity and compatibility metadata → atomically publish checkpoint manifest → resume or suspend according to request.

A crash before publication yields an incomplete capture that cannot be routed as restorable. A checkpoint referencing unavailable mutable volumes is incomplete unless those volume versions are pinned. Garbage collection protects active references, in-progress captures and legal/retention holds; deletion is tenant-scoped and audited.

### 9.3 Restore protocol and authority

Resolve checkpoint under current authorization; verify bytes and profile compatibility; check current approval, policy and cancellation state; reserve resources; assign a new execution/attempt and fencing epoch; create isolated mounts/network; restore while guest egress remains blocked; run the trusted rebind hook; only then admit new effects.

Rebinding refreshes execution identity, principal references, broker credentials, leases, connection IDs and entropy as required by the profile. A copied guest token or old network session must not bypass current authorization. An environment that cannot safely rebind stale guest authority MUST NOT advertise protected memory restore/fork; use a logical restart instead.

Snapshots can contain secrets even when API keys are brokered: browser sessions, cached data and memory remain sensitive. Encrypt and tenant-scope captures; never place secret-bearing checkpoints in shared warm pools. A remote provider hosting plaintext RAM remains in the trust boundary unless a separately verified confidential-compute profile says otherwise.

### 9.4 Fork and experimental fairness

A fork plan binds a parent state, experiment definition, arm IDs, independent trials, carved aggregate budgets and verifier. Each child gets a private workspace and fresh attempt identity. Child grants are attenuation, not copies of unlimited parent authority. Budget and revocation remain host-authoritative across snapshots.

Default branch effects are local or mocked. Real external writes require separate approval and effect IDs; a snapshot does not roll back an email, payment, database mutation or cloud provisioning request. Even a production HTTP read can leak data or have side effects and must remain policy-bound.

For comparable experiments, record base environment/workspace, model/program versions, seeds, backend profile, warm/cold/checkpoint mode, cache reuse, network mode, task/arm/trial and all resource spend. Changing the execution backend is an experimental factor, not hidden noise. Retain failed and canceled attempts in the denominator.

## 10. Native backend requirements

### 10.1 Legacy interpreter adapter

Preserve reference interpreter execution and existing fail-closed verdicts, scoped wrappers, virtual-clock behavior and approval tests. Fix concurrency-sensitive temporary resources and bounded output as part of reuse. The adapter advertises only its observed scoped `.ax` behavior. It cannot satisfy arbitrary native confinement, hardware isolation, memory capture, desktop or remote guarantees. [E03–E06]

`PrincipalHandle(0)` in the existing implementation is an implementation-local placeholder, not a global principal identity. A service adapter must retain principal-to-grant binding in supervisor state rather than expose that value as authority across requests. [E05]

### 10.2 Firecracker/Linux profile

Extract `axon-vm` into reusable launch/configuration/result modules without changing CLI meanings, exit codes, attestation/quorum controls, existing schemas or source-era tests. New daemon/service code must return values rather than call `process::exit` inside library paths. [E11, E18]

The first protected microVM profile MUST explicitly identify a supported **Linux guest image and init protocol**. Existing launcher policy delivery favors kernel command-line data without a NIC; Linux guest init reads MMDS. Reconcile that protocol mismatch deliberately and test the actual image pair. Do not weaken fail-closed guest policy handling to make it boot. [E15, E19–E21]

Required gates include real registered workload execution, loaded executable/content binding, secure VMM launch ownership, restricted API sockets, host cgroup/UID boundaries, disk/workspace projections, guest policy enforcement, independent deadline/descendant control, output limits, default-deny egress, result provenance and failure cleanup.

The current payload contains effect names and token caps rather than the complete OS path/host grant. Translation must either enforce every requested scope through a tested mechanism or refuse the profile. Do not downgrade path-specific authority into `FS=true` or host-specific authority into `Net=true`. [E07, E13]

The current source explicitly substitutes balloon settings for production jailer-style controls. That is not evidence of full host resource/UID isolation. Attestation of a kernel does not prove the right job ran. Guest success must be coupled to a workload receipt and independent check result; the kernel demonstration's clean halt cannot pass a registered workload gate. [E15, E18, E21]

### 10.3 WASM host extension

Keep the browser `axon-wasm` ABI intact. An optional hosted WASM profile needs an explicit pinned host engine, bounded instance memory/fuel or interruption, host-call allowlist, deterministic-enough clocks/randomness where required, output limits and a fresh instance boundary per protected run. It must exercise the real `.ax` interpreter/WASM artifact, not only an empty test module.

WASM code confinement does not constrain a permissive host import. Untrusted guest code must never share the supervisor's authority or arbitrary filesystem/network host implementation. Cancellation and host-call deadlines must work even when a module loops or blocks in an import. General host snapshots require their own evidence and cannot be inferred from Axon's cooperative fiber suspend/resume.

## 11. Egress, secrets, imports and trust

Initial protected work uses offline fixtures and no egress. Later egress requires a trusted broker outside the guest plus enforcement that prevents bypass. Destination, method/path, scope, redirect and credential policy are checked at the actual boundary; raw IPs, DNS changes, proxies, alternate protocols, metadata endpoints and existing connections are included in tests.

Credential injection is permitted only for explicitly integrated protocols/services where destination and request semantics can be authenticated. A generic TLS byte tunnel cannot magically inject HTTP credentials. Other cases use short-lived scoped credentials with a documented exposure profile, or refuse. Never claim that brokered credentials eliminate all sandbox secrets or external effects.

The broker binds each request to principal, attempt, current epoch, grant and remaining reservation. It keeps credentials and audit authority outside worker-accessible memory and disks. Provider API credentials remain in the trusted driver service, not worker images. Artifact upload, package download, preview URL creation and desktop streaming are all separately scoped export/access operations.

Protected channels, caches, snapshots and storage are tenant-isolated. Backend logs and guest output are untrusted and may contain secrets or forged result-looking data. Redact without destroying authoritative accounting; record intentional omissions. Telemetry must not become an unbounded data-exfiltration sink.

## 12. Budgets and measurement

Reuse existing calls/tokens/cost semantics; add a separate versioned compute resource contract for CPU quota, memory, storage, output, wall-time, GPU resources, concurrent jobs and provider costs. The current `Budget` has only calls, tokens and `cost_micro`; it cannot represent all these constraints as-is. [E07]

A trusted reservation ledger enforces, on each relevant axis:

`settled_consumption + active_reservations + unresolved_liability <= authorized_limit`.

Execution changes a reservation into measured consumption plus remaining reserved capacity; it never double-counts the same charge. Child allocations carve the parent budget atomically; ten forks do not each inherit the full parent's remaining funds. Snapshot/restore cannot rewind the ledger.

Use explicit units, checked integer arithmetic and currency/price-version bindings. Admission prices may be estimates; final usage distinguishes `estimated`, `metered`, `provider_reported`, `settled` and `unknown`. Storage, idle resources, failed boots, retries, losing branches, egress and cleanup are counted. Unknown invoice exposure cannot be marked free.

Hard local resource controls and dispatch ceilings bound known execution; uncertain external billing cannot be represented as an exact hard monetary guarantee without a supporting provider contract. Requests missing a required bound are denied.

Optimize completed-task economics, not a single boot time. Measure cold create, warm attach, resume, checkpoint, fork, ready-to-use, end-to-end completion, verified quality, outcome uncertainty, cancellation lag and total cost. A cheap provider with more retries may cost more per accepted task. Learned ranking is admitted only after hard filtering and a fixed baseline evaluation.

## 13. Cortex, Axon evidence and ASI OS integration

Introduce `CheckExecutor` into the runner so a registered check becomes a typed compute job rather than a direct `Command::output`. Preserve the `Authorized` action boundary and grader-not-patchable rules. The worker is not the verifier and stdout is not the supervisor's authoritative result channel. [E29, E31]

A trusted, framed result channel identifies the attempt and executable, records exit/timeout/transport outcome, and validates the expected test inventory/schema. An exit code of zero, missing test summary, zero matching checks, duplicate verdict, late forged summary or worker-provided “passed” cannot produce independent verification.

Append execution evidence keyed by `TaskId / ArmId / TrialId / AttemptId / OperationId`, with links to the existing episode, workspace observation, effective authority digest, environment, backend profile, immutable output and verifier receipt. This is one provenance system with typed nodes and relations, not a collection of disconnected graphs. Graph lineage is evidence, not permission.

Do not rewrite historical `axc1:` hashes, `axon-os-record/1`, `axon-vm-run/2` or episode JSON in place. Use an `acf-execution/1` sidecar envelope referencing existing digests. Any stronger wire format has a new schema/version and explicit reader migration. Old missing fields remain unknown, never retroactively verified. [E09, E18, E26–E27]

The ASI OS `Computer` presentation maps to an execution/workspace/lease. A stateless pure WASM job need not pretend to have a terminal or desktop. Later human attach grants are read-only or read/write separately, short-lived and exclusive where needed. Taking control fences agent input; releasing control rechecks policy and records provenance.

The fabric may publish measured runtime capabilities to CX-34. The cognitive scheduler keeps ownership of goals, actions, model choices and experiments. The compute broker performs constrained placement and lifecycle management. Neither learns away a safety requirement. [E36]

## 14. Remote providers and portability

Implement a fake remote protocol only to test contracts and recovery, followed by **one** separately selected real provider. This package endorses no current vendor capability: earlier vendor claims are not evidence in this source-based specification.

A remote driver needs exact profile/version binding, authenticated transport, scoped provider credentials, residency and data export approval, artifact import/export integrity, documented retry semantics, quota enforcement, observed stop/deletion, usage reconciliation and conformance evidence. Provider SDK auto-retries must be surfaced and constrained, not hidden beneath operation IDs.

Logical reconstruction uses exported S0/S1 plus a reference to S3 when allowed. Cross-provider RAM conversion is unsupported unless an adapter supplies explicit compatibility evidence. Remote attestation/metrics are labeled by origin and strength. Provider termination APIs do not prove retained snapshots or backups were deleted; retention/deletion assurances are separately recorded.

No host outage may silently redirect secret workloads to an unapproved cloud. No provider response may grant itself additional capabilities. Removing a provider invalidates its active capability claims and triggers safe draining/reconciliation rather than blind replay elsewhere.

## 15. Protocol encoding and example scope

The JSON examples and schemas shipped here cover a minimal request, profile and execution-receipt envelope. They are contract fixtures, not a full SDK or daemon API. Examples marked synthetic contain no real approval or runtime evidence.

Use strict UTF-8 JSON, reject duplicate keys after escape decoding, reject unknown closed-schema fields, reject non-finite numbers and invalid Unicode, and bound payload size/depth. Numeric counters in this first wire format are integers within the interoperable range 0 through 9,007,199,254,740,991; byte/quota bounds may impose smaller limits. Larger future counters require a versioned representation, not silent rounding.

For new `acf1:` content identity, canonicalize valid JSON with keys sorted by Unicode scalar order, compact separators, UTF-8 strings without Unicode normalization, shortest decimal integer representation and JSON-required string escaping. No floats, duplicate keys or invalid Unicode are admitted. Hash the canonical bytes with SHA-256 and prefix `acf1:`. This algorithm does not replace existing `axc1:` identity rules and provides no issuer authentication.

Artifact transport authorization, schema validation, content identity and issuer trust are four different checks. Passing one does not imply the others.

## 16. Acceptance, rollout and rollback

`build/ACCEPTANCE_GATES.md` is the product-gate inventory. Every gate starts `NOT_RUN`. Passing the package validator means the proposal is internally consistent; it does not admit any runtime.

Rollout order:

**M0 — source-aligned contracts.** Pin source and existing behavior, add typed profiles, strict request parsing, authority convergence and feature flags. No provider is marked production-verified from inspection.

**M1 — protected native vertical slice.** Add durable reservation/lifecycle state, safe workspaces, hardened Linux execution and a fabric-backed registered Cortex check. The same task passes without touching the operator workspace; hostile and crash fixtures fail closed.

**M2 — logical experimentation.** Branch immutable workspace versions, run isolated alternatives, independently evaluate and promote with CAS and aggregate budgets. No memory fork is required.

**M3 — independently gated extensions.** Memory checkpoints/forks, bounded WASM host, egress broker and one remote driver each ship only for their tested profile. Desktop/GPU and broader OS support remain capability-specific.

**M4 — measured routing.** Rules first; shadow comparison, fixed workload evaluations and operator-approved policy versions precede any learned routing. The hard admission filter is unchanged.

Feature-off preserves old CLI behavior and stored records. A protected workload never rolls back to weaker confinement; it is paused/refused until an admissible backend is available. Before rollback, drain or reconcile active executions, retain their budgets and artifact leases, and keep readers for the new envelope until retention expires.

## 17. Definition of done

A release is done only when its required product gates actually ran on the declared host/backend profile, negative fixtures produced no unauthorized effects, old behavior/schema tests pass, execution and verifier outcomes remain distinct, repeated trials are independently attributable, lifecycle failures reconcile safely, artifact promotion is atomic, and cost/liability accounting includes failed and abandoned work.

This proposal package is done when the source evidence is traceable, assumptions and absent implementations are explicit, contracts/examples validate, every work package has dependencies and nonvacuous product gates, and the bootstrap prompt prevents overwriting existing work. **The package does not claim that the fabric has been implemented.**
