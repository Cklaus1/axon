# Source review — Axon Compute Fabric readiness

**Review basis:** `axon-ai-context-20260924(1).zip`, reporting snapshot `4cceb89` dated September 24, 2026. The archive was read locally, not replaced with a live repository. The commit label comes from the bundle README; its Git object was not independently verified. Archive and per-file SHA-256 values are recorded in `manifests/source_manifest.json`.

**Scope:** targeted static review of the runtime, supervisor, grant and record interfaces; Firecracker launcher and guest boundary; WASM and host interfaces; Cortex snapshots, action authorization and test execution; and the included CX-03/CX-04/CX-13/CX-34 contracts. This is not an exhaustive audit of all 1,213 archive entries. The evidence index identifies 36 inspected source/specification ranges. Presence of a test or historical comment is not a test run in this review.

## 1. Conclusion

Build the Compute Fabric as an extension around **existing authority**, not as a replacement runtime. There is already enough structure to avoid a greenfield orchestration framework: `axon-os::Runtime`, `supervisor::run`, scoped `Grant`, `AxonHost`, Cortex's `Authorized` action, `WorkspaceSnapshot`, episode evidence and the Firecracker launcher.

The earlier conceptual answer overstated current readiness. This tree does not show three finished, interchangeable WASM/microVM/full-VM services. `axon-vm` is the Firecracker microVM launcher itself; `axon-wasm` is a browser-oriented interpreter build; the custom guest kernel explicitly stops short of loading the interpreter and VFS. Machine checkpoint/fork APIs and a durable workspace content service remain additions.

The recommended first milestone is one **registered Cortex check executed in a verified isolated Linux profile**, with immutable inputs, aggregate reservation, cancellation, an attributable result and unchanged operator workspace. Logical branches follow. RAM forks and vendor adapters are not first-release blockers.

## 2. What to reuse

| Existing mechanism | Evidence | Integration decision |
|---|---|---|
| `Runtime` with declared effects, mint and scoped run | `runtime.rs:28–53` [E03] | Preserve; add a compatible bridge, not a second unrelated `.ax` runner. |
| Central approval and grant intersection | `supervisor.rs:38–93` [E04] | All new execution/lifecycle entry points must converge on equivalent authorization. |
| Scoped path/host grants and calls/tokens/cost budgets | `grant.rs:62–99` [E07] | Preserve algebra; add compute limits and reservation semantics explicitly. |
| Interpreter host effects | `host.rs:24–29,84–108` [E24–E25] | Reuse `AxonHost`; do not overload it into VM lifecycle management. |
| Firecracker launch and typed guest outcome | `axon-vm/main.rs:2237–2482,1514–1546` [E14–E18] | Extract a library with unchanged CLI/exit schemas, then harden/profile it. |
| Cortex authorization token | `runner.rs:109–127` [E29] | Keep the authorized-action boundary while injecting a check executor. |
| Snapshot observation and lineage hash | `axon-cortex/lib.rs:91–140` [E27] | Preserve its meaning and hash; link to a new durable content version. |
| Run records and episode identity rules | `record.rs:19–71`, `axon-cortex/lib.rs:36–66` [E09,E26] | Use sidecar references; identity is not authenticity. |
| H0/H1/H2/K roadmap and transactional action model | CX-13 and CX-03 [E33–E34] | Extend these contracts rather than replacing them with vendor terminology. |

## 3. Findings requiring action

Priority is relative to admitting untrusted fabric workloads, not an assertion that an exploit was demonstrated. “Observed” means visible code or an explicit source limitation; “inferred risk” means the relevant failure was not reproduced here.

### F01 — The custom guest kernel is not yet a general workload executor

**Observed; blocks advertising general execution.** `crates/axon-guest-kernel/src/enforce.rs:316–342` says the interpreter ELF loader and VFS are remaining K5 work. Its FS-allowed path prints that the program would proceed and halts; its denied path exercises a real syscall gate. [E21]

**Impact:** a clean VM exit can describe completion of the enforcement demonstration rather than execution of the requested task. The fabric must identify `axon_kernel_demo` separately and require a workload-specific completion/result gate. The first protected hosted profile should explicitly validate a Linux guest path. Do not discard the kernel work; keep it an independently gated research backend.

### F02 — Interpreter containment is not arbitrary-native-process confinement

**Observed boundary; high priority for new native workloads.** `AxonCoreRuntime` creates an Axon wrapper and invokes the interpreter as a subprocess. That is a meaningful language-level gate, but the shown path does not establish a general host filesystem/network/cgroup enclosure for arbitrary child executables. CX-13 itself warns about this distinction. [E03–E06,E33]

**Impact:** routing a registered native build or tool here solely because it accepts a process command would overstate protection. Keep a truthful `process_scoped` profile and require a verified enclosure for untrusted native code. Do not change existing trusted/interpreter workflows merely to align names.

### F03 — Firecracker lifecycle cleanup is concentrated on the normal path

**Observed control flow; inferred leak risk; not fault-injected.** `run_in_firecracker` spawns the VMM, then performs fallible socket/API/configuration operations with `?`. Normal cleanup occurs at the end. The reviewed function has no owning cleanup guard covering all those early returns. [E14–E17]

**Impact:** a service/daemon must not assume failed provisioning leaves no VMM, relay or socket behind. Extract ownership into a guard and inject failures after each acquisition. Record cleanup failures as live obligations. This finding does not claim a leaked process was observed in this environment.

### F04 — Per-process temporary names are unsuitable for concurrent service requests

**Observed naming; concurrency risk before daemonization.** The interpreter wrapper path uses process ID plus program stem (`runtime.rs:539–546`). The VM relay path uses the launcher process ID (`main.rs:2329`); guest CID is fixed in the shown request. [E05,E15]

**Impact:** the current one-shot CLI shape must not be assumed safe when multiple attempts run inside one host process. Use per-attempt private resource directories and runtime-scoped identifiers. Test identical stems and simultaneous launches. A fixed CID can be valid in independently isolated VMMs; the requirement is to prove the relevant namespace isolation, not to declare the constant inherently wrong.

### F05 — Linux init and the no-NIC launcher need an explicit policy-delivery contract

**Observed source mismatch; integration outcome not run.** The launcher comments say its primary policy channel is the kernel command line and no NIC is configured. Linux `axon-guest-init::read_mmds` gets policy from MMDS. The custom guest kernel reads command-line policy. [E15,E19–E21]

**Impact:** the guest kind is part of the backend profile. A Linux image cannot inherit the custom kernel's boot-policy assumptions. Implement/test a matching delivery protocol without disabling the guest's fail-closed behavior. Missing boot artifacts mean this review cannot determine which image is installed on the operator's actual host.

### F06 — VM policy is not a lossless projection of the OS grant

**Observed representation gap; high-priority admission requirement.** `MmdsPayload` contains effect names, token budget, source hash and seccomp bytes, while `Grant` contains scoped path/host allowlists and other policy. The shown VM payload has no corresponding full path/host representation. [E07,E13]

**Impact:** do not translate a path-scoped grant to a broad FS permission and call it equivalent. A profile must enforce each axis through the guest, host mounts/network, broker or another tested boundary, or refuse. `AXON_SOURCE_HASH` is explicitly described in Linux guest-init as attribution, not a current loaded-image check. [E19]

### F07 — Current workspace snapshots are evidence, not durable machine state

**Observed; design gap rather than a defect in the existing type.** Cortex snapshots map paths to hashes, scope and parent references; snapshot collection hashes existing files and records absence. `stage_copy` copies immediate regular files only. [E27–E28,E32]

**Impact:** preserve this observation contract. Add an actual content store, safe materializer, scope/completeness metadata and atomic promotion. Do not rename the existing structure into a restorable filesystem snapshot or promise complete repository backups.

### F08 — The Cortex test-run path needs a trusted execution/result boundary

**Observed; high priority for fabric integration.** `Runner::run_tests_json` calls `Command::output()` directly, then parses JSON-looking lines from combined stdout/stderr. The function shows no bounded execution or environment projection, and its own comment acknowledges that a test body can print a matching JSON object. [E31]

**Impact:** add `CheckExecutor`, enforce the full job policy, bound output/time, and obtain a framed supervisor-attributable execution receipt. Test missing, forged, duplicate and zero-match results. Do not infer that every path reaching this function is publicly exploitable; this is a source-level boundary needing explicit protection before untrusted service use.

### F09 — Existing episode naming is not unique across repeated trials

**Observed; high priority for experiment correctness.** `runner.rs:621–624` derives the episode ID from target path, target symbol and hidden check. The same request repeated produces the same name by design. [E30]

**Impact:** retain that name as a semantic grouping identifier and introduce `TrialId`, `AttemptId` and `OperationId`. Do not use task/arm alone as a billing, idempotency or result key. Replay identity and new experiment identity are different.

### F10 — Hosted WASM requires additional runtime enforcement evidence

**Observed packaging/surface; missing capability is not proof of a broken implementation.** `axon-wasm` is a browser-oriented cdylib with alloc/eval/output ABI. `AxonHost` supplies virtualized effects; it is not itself a fuel-limited multi-tenant WASM service. [E22–E25]

**Impact:** preserve browser behavior. Add a separate bounded host profile and test real Axon execution, memory/fuel interruption, host imports, isolation and cancellation before routing protected jobs there.

### F11 — Machine checkpoint, suspend and fork are not in the inspected launcher API

**Observed API/search result.** The inspected VM command surface and orchestration implement run/attestation/principal/quorum/chain behavior, with an `InstanceStart` action. No machine snapshot-load/create, pause/resume or fork path was found in those runtime modules. [E12,E14–E18]

**Impact:** the spec defines these as future negotiated capabilities. A logical workspace branch can deliver useful A/B execution first. Existing language-level resume/fiber features must not be relabeled as full VM checkpointing.

### F12 — Aggregate resource reservation must be added, not inferred from grant intersection

**Observed type boundary; high priority before fan-out.** The current OS `Budget` has calls, tokens and `cost_micro`. Intersecting a grant is not equivalent to atomically reserving CPU/memory/storage/money across independent children. [E04,E07]

**Impact:** add a durable ledger for settled usage, reservations and unresolved liability. Forking or restoring cannot duplicate remaining budget or undo settled spend. Unknown remote billing remains explicit.

### F13 — Production host isolation is not established by the launcher comments

**Observed explicit limitation.** `main.rs:2362–2374` describes jailer-style controls as production work and uses balloon settings instead. [E15]

**Impact:** cgroup/UID/API-socket/device and cleanup controls need target-specific evidence. Hardware isolation, host resource enforcement, software measurement and confidential-compute guarantees are separate claims.

### F14 — Version and evidence status must remain honest

**Observed package contents and executed documentation gate.** Only the v0.15 vendored Cortex package appears in this archive. Its package gate reports 247 proposed product gates, all `NOT_RUN`, and an empty execution registry. Source implementations also exist; an empty package registry does not mean there is no code. Conversely, source code and historical tests do not prove those package gates passed. [E01,E33–E36; executed log]

**Impact:** use a standalone ACF addendum and rebase it onto the actual latest checkout. Do not overwrite v0.20 files, invent updated gate status or claim end-to-end readiness based on package validation.

## 4. Checks actually executed

| Check | Measured result | What it does not prove |
|---|---|---|
| Safe archive extraction and manifest inventory | 1,213 file entries; hashes recorded | No authenticity guarantee for the supplied source/commit label. |
| Python source parsing | 37 files, all parsed | No Python behavior tests. |
| TOML parsing | 25 files, all parsed | No dependency resolution or Rust build. |
| Shell syntax (`bash -n`) | 126 `scripts/*.sh` files, all passed | No execution of those product gates. |
| Existing `scripts/cortex_package_gate.sh` | PASS; 113/113 pinned files; internal validator PASS | Executed **zero** product gates. |
| Rust compile/test | NOT_RUN | `cargo` and `rustc` are absent. |
| Firecracker/guest integration | NOT_RUN | Firecracker and `/dev/kvm` are unavailable. |

The source checks are in `reports/source_checks.json`; the gate transcript is in `reports/existing_cortex_package_gate.log`. The extracted source was not patched. The existing gate wrote its report under the extracted tree's `target/` directory; that generated report is not a source change.

## 5. What the review changes in the proposed architecture

The framework becomes **engine × enclosure × placement**, not three implied complete runtimes. Authority stays in `axon-os`; `AxonHost` stays the per-effect interface; Cortex retains its cognitive/action/evaluation roles; the fabric adds execution placement, lifecycle and durable state. Workspace portability is capability- and export-constrained, not universal. Checkpoints are optional backend artifacts, not the root of all agent state. Remote vendors are optional adapters after native conformance, not architectural dependencies.

No finding requires restarting Axon from scratch. The main integration work is turning the existing components into a truthful, bounded, recoverable service without dropping their current safety and evidence contracts.
