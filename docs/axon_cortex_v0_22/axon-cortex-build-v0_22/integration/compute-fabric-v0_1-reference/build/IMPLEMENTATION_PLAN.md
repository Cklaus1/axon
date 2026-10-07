# Implementation plan

## Preservation rules

This is an additive proposal, not a patch. Rebase against the real latest checkout, including dirty/untracked work and any Cortex v0.20/MiCode updates that are absent from the supplied archive. Do not replace an existing source tree with this bundle. Proposed paths below are not claimed to exist.

Keep interpreter-first development, existing CLI/result schemas, capability algebra, approvals, independent verifiers, and all current regression fixtures. Do not enable the known `codegen` + `serde-json` feature combination. No source requirement is satisfied by changing a test expectation to agree with an unsafe implementation.

## Milestones

M0 pins and defines. M1 delivers one protected native Cortex check. M2 adds useful logical A/B branches. M3 gates independent execution/network/provider extensions. M4 measures routing and scoped operator interfaces. T05 can proceed in parallel after intake, but protected microVM dispatch waits for all T06 dependencies.

## ACF-T00 — Rebase and preserve the source baseline

**Milestone:** M0. **Status:** Not started. **Dependencies:** None.

Read the actual checkout, dirty/untracked work, current Cortex/MiCode package versions, AGENTS.md, and source bindings. Produce a delta map; do not overwrite existing work.

**Proposed edit surface:** governance/reviews/ACF-intake.md; source-binding report.

**Required gates:** ACF-G00, ACF-G01.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T01 — Contracts, profiles and authority convergence

**Milestone:** M0. **Status:** Not started. **Dependencies:** ACF-T00.

Define versioned ComputeJob/profile/receipt contracts and a supervisor-owned admission path. Keep Runtime, AxonHost, existing Grant semantics and old manifests intact.

**Proposed edit surface:** optional crates/axon-compute/src/{protocol,profile,admission}.rs; axon-os/src/fabric.rs.

**Required gates:** ACF-G02, ACF-G03, ACF-G04, ACF-G05, ACF-G06, ACF-G07.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T02 — Safe legacy process adapter

**Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T01.

Preserve interpreter behavior; use unique private staging, bounded output/deadline and truthful process_scoped profile. No arbitrary native confinement claim.

**Proposed edit surface:** axon-os/src/runtime.rs and runtime tests.

**Required gates:** ACF-G08, ACF-G09, ACF-G10.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T03 — Durable lifecycle, reservations and reconciliation

**Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T02.

Implement journal-before-effect, idempotency, fenced leases, aggregate resources, cancellation and outstanding cleanup/billing obligations.

**Proposed edit surface:** compute lifecycle/journal/budget modules; supervisor integration.

**Required gates:** ACF-G11, ACF-G12, ACF-G13, ACF-G14, ACF-G15, ACF-G16.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T04 — Immutable workspace store and safe materializer

**Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T03.

Store and verify actual file content and metadata, isolate tenants and worktrees, preserve scoped observation digests and pin retention references.

**Proposed edit surface:** compute workspace/artifact modules; Cortex observation projection.

**Required gates:** ACF-G17, ACF-G18, ACF-G19, ACF-G20.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T05 — Extract axon-vm library without CLI drift

**Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T00.

Move reusable launch/config/result logic from main.rs into a library; own cleanup resources. Preserve existing CLI, exit codes and attestation gates.

**Proposed edit surface:** crates/axon-vm/src/lib.rs plus modules; existing main.rs remains thin.

**Required gates:** ACF-G21, ACF-G22.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T06 — Harden and verify the Linux microVM profile

**Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T03, ACF-T04, ACF-T05.

Pin a working Linux guest/init policy path; enforce full scopes and host resources; prove real registered workload execution and no-NIC/offline confinement.

**Proposed edit surface:** axon-vm Linux backend; axon-guest-init protocol; host-image build and KVM tests.

**Required gates:** ACF-G23, ACF-G24, ACF-G25, ACF-G26, ACF-G27, ACF-G28.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T07 — Cortex registered CheckExecutor vertical slice

**Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T03, ACF-T04, ACF-T06.

Inject a typed check executor into Runner, preserving action authorization and protected grader semantics. Link independent execution/verifier receipts to episodes.

**Proposed edit surface:** axon-cortex/src/runner.rs; new executor seam; cortex-policy-adapter compatibility tests.

**Required gates:** ACF-G29, ACF-G30, ACF-G31, ACF-G32, ACF-G33.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T08 — Logical branches and atomic promotion

**Milestone:** M2. **Status:** Not started. **Dependencies:** ACF-T04, ACF-T07.

Run alternatives from one durable base with separate trials/worktrees, carved budgets, fixed verifiers and fenced CAS promotion. No real external side effects by default.

**Proposed edit surface:** compute branch/promotion modules; Cortex experiment evidence.

**Required gates:** ACF-G34, ACF-G35, ACF-G36, ACF-G37.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T09 — Optional machine checkpoint/restore/fork

**Milestone:** M3. **Status:** Not started. **Dependencies:** ACF-T06, ACF-T08.

Implement only tested profile combinations, capture completeness, durable publication, compatibility, current-authority rebind and restore failure cleanup.

**Proposed edit surface:** axon-vm checkpoint backend; compute checkpoint manifests and KVM conformance.

**Required gates:** ACF-G38, ACF-G39, ACF-G40, ACF-G41.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T10 — Optional bounded hosted WASM profile

**Milestone:** M3. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T03, ACF-T07.

Select/pin the host engine separately; preserve browser ABI; prove real Axon parity, interruption, memory and host-import constraints.

**Proposed edit surface:** feature-gated WASM host adapter; existing axon-wasm/browser tests.

**Required gates:** ACF-G42, ACF-G43, ACF-G44.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T11 — Optional scoped egress and secret broker

**Milestone:** M3. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T03, ACF-T06.

Keep service credentials outside guests where protocol supports brokering; enforce actual egress boundaries and snapshot/session sensitivity.

**Proposed edit surface:** trusted network/secret broker; driver host enforcement.

**Required gates:** ACF-G45, ACF-G46, ACF-G47.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T12 — One optional remote driver after protocol conformance

**Milestone:** M3. **Status:** Not started. **Dependencies:** ACF-T03, ACF-T04, ACF-T07, ACF-T11.

Use a fake remote only for protocol tests, then select/pin one real provider and validate export, retries, cancellation, usage and deletion semantics.

**Proposed edit surface:** isolated feature-gated remote adapter; recorded source/version intake.

**Required gates:** ACF-G48, ACF-G49, ACF-G50, ACF-G51.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## ACF-T13 — Measured routing and scoped operator surfaces

**Milestone:** M4. **Status:** Not started. **Dependencies:** ACF-T08, ACF-T12.

Keep hard admission outside learning, benchmark completed-task economics and add attach/takeover only for specifically verified profiles.

**Proposed edit surface:** compute routing metrics; optional ASI OS resource UI/API.

**Required gates:** ACF-G52, ACF-G53, ACF-G54.

Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.

## Regression and deployment procedure

Run baseline tests before changes. Record unavailable prerequisites explicitly; do not fabricate a green baseline. Use focused Rust tests during the build, and include the interpreter-only build, `axon-os` tests, `axon-vm` tests, `axon-cortex` tests, existing policy-adapter tests and the applicable host gates in final validation. Run package integrity validation separately from product gates.

Introduce an opt-in feature/service entrypoint. First compare trusted offline fixture outcomes, then admit one tested protected profile. Keep the default route unchanged until the protected vertical slice passes. Never use an old weaker route as rollback for an already protected request. Drain/reconcile active jobs before disabling a backend and keep result/sidecar readers until retained data expires.

## Candidate commands to run on a suitable development host

These are existing crate-level commands, not commands executed by this package or a promise that the checkout compiles. Add the new crate tests only after the implementation exists.

```bash
cargo build -p axon-core --no-default-features --bin axon
cargo test -p axon-os
cargo test -p axon-vm
cargo test -p axon-cortex
cargo test -p cortex-policy-adapter
bash scripts/cortex_package_gate.sh
```

Review feature selection against the target checkout before broad workspace builds. KVM, WASM/browser and provider tests require their actual infrastructure; missing infrastructure must not be hidden by a passing mock.
