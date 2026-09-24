# Adversarial design review and incorporated resolutions

This is a second-pass challenge of the **proposal**, not a penetration test. Each row identifies a failure an implementation must resist. The resolutions are incorporated into ACF-01; product tests remain `NOT_RUN`.

| Challenge | Failure without an explicit rule | Resolution in the specification |
|---|---|---|
| Is a new fabric another authority kernel? | Provider APIs bypass approval or manufacture grants. | Keep `axon-os` authority; new entry points converge on it. Registry support is not permission. |
| Are engine and security tier being conflated? | A WASM or remote label is accepted as a hardware boundary. | Bind exact engine/enclosure/guest/OS/architecture/profile combinations. |
| Does a successful kernel demo mean the task ran? | Clean halt is counted as workload success. | Distinguish demonstration guest; require executable binding and a real task/result gate. |
| Can the new adapter run an arbitrary shell through a fake `.axjob`? | Unknown effects are silently trusted. | Separate explicit `ComputeJob`/registered-check protocol; keep old program semantics. |
| Does source scope disappear at the VM boundary? | Scoped FS/net grants become broad effect booleans. | Require lossless enforcement or explicit refusal on every policy axis. |
| Can retries provision two machines? | A timed-out create is sent to a second provider. | Journal operation before side effect, deduplicate same inputs, reconcile uncertain outcome. |
| Can a restored image reuse revoked credentials? | Old principal/token state bypasses revocation. | Current host epoch, blocked egress, trusted rebind, fail closed when rebinding is unsupported. |
| Can cloning duplicate money? | Every child inherits the parent's full budget. | Atomic carved reservations; immutable external accounting survives checkpoint/restore. |
| Can two identical task/arm trials overwrite each other? | Results and charges share one deterministic ID. | Task, trial, attempt and operation IDs have different roles. |
| Can a losing branch already have acted on production? | Promotion semantics imply rollback of external effects. | Offline/local branch default; independent authority and idempotency for real effects. |
| Does a hash authenticate an outcome? | Worker invents valid-looking hashes and passes. | Separate content identity, issuer authentication, effect observation and independent verification. |
| Can a test body forge the grading transcript? | JSON-looking stdout changes a result. | Framed trusted result channel; exact test inventory, missing/duplicate/zero-match rejection. |
| Can a snapshot hide secret credentials? | Shared warm pool leaks browser/session state. | Snapshot classification, tenant encryption, no secret-bearing shared pool, explicit export policy. |
| Does a proxy keep every secret outside the guest? | TLS tunnel cannot inject authenticated HTTP credentials. | Broker only tested protocol integrations; scoped exposed credentials or refusal elsewhere. |
| Does checkpoint portability survive incompatible OS/hardware? | Logical migration is promised to any backend. | Compatibility/export/dependency preflight and explicit unsupported result. |
| Can a stale writer promote after another branch wins? | Candidate overwrites newer accepted work. | Fenced lease and CAS on the expected base; rebase and reverify on conflict. |
| Can a worker escape via descendants or inherited descriptors? | Killing parent is treated as killing job. | Host-bound enclosure and deliberately detached-descendant termination tests. |
| Can a failed launch leak resources or money? | Cleanup exists only at the bottom of a happy path. | RAII/owned lifecycle, fault injection per acquisition, unresolved cleanup liability. |
| Can stricter enforcement disappear behind a feature flag? | Feature-off falls back to an unsafe subprocess. | Preserve old trusted behavior but refuse protected work without an eligible profile. |
| Can native and browser builds inherit provider dependencies? | GPU/vendor/LLVM dependencies contaminate minimal builds. | Pure contracts, optional drivers, interpreter-first feature/build regression gates. |
| Can operator replay double external effects? | Historical replay is used for crash recovery. | Reconcile effects; explicit deterministic re-execution remains a separate operation. |
| Can stale “verified” profile data keep routing new jobs? | Backend/config changes invalidate tests silently. | Version/config-bound evidence, expiry, withdrawal, fail-closed scheduling. |
| Can package validation be represented as a product test? | All gates appear green without a runtime. | Separate package report, source checks and per-profile product evidence; all new gates NOT_RUN. |
| Can learned routing optimize away safety? | Cheap but nonconforming backends gain traffic. | Hard authority/profile filter precedes any learned ranking; shadow evaluation first. |

## Residual decisions deliberately left to implementation intake

**Supported Linux target and image pair.** The uploaded archive is not a deployed host inventory. Pin actual VMM/kernel/rootfs/init/build artifacts and reproduce the workload/security gates on the target. The proposed custom-kernel profile remains research-only until its own full-execution evidence exists.

**Journal and workspace storage technology.** The spec requires transactional reservations, operation uniqueness, CAS, crash safety and immutable retrievable contents; it does not select a new database without measuring existing deployment constraints. A simple single-host transactional implementation is preferred to introducing distributed coordination prematurely.

**WASM host and first remote provider.** Source review cannot verify a current vendor SDK or choose a hosted WASM engine implementation that is absent from this tree. Select and pin them during their own work package, then run the same negative and recovery contracts. Do not reuse the earlier marketing comparison as capability evidence.

**Memory checkpoint support.** Exact disk/RAM/device and credential-rebind semantics must come from the implemented profile. Logical branches remain useful and supported independently. A backend that cannot satisfy protected RAM restore must say so.

**Approval authenticity.** Existing approval paths are reused for compatibility, but stronger remote claims require an authenticated authority mechanism binding the full compute envelope. No local hash format receives a silent security upgrade.

These are bounded implementation choices, not reasons to block the contracts or the first native vertical slice. Their outcomes must be recorded in the target checkout before the corresponding profile is enabled.
