# Implementation-agent bootstrap prompt

You are implementing the **Axon Compute Fabric**, using this proposal as an additive, source-bound design input. Do not interpret proposal/schema validation as implementation evidence.

## Read and preserve before changing

Read the target checkout's `AGENTS.md`, current status/build constraints, `axon-os` runtime/supervisor/grant/record paths, `AxonHost`, the Firecracker launcher and both guest paths, Cortex's authorization/snapshot/check runner, and the active CX-03/CX-04/CX-13/CX-34 equivalents. Then read ACF-01, SOURCE_REVIEW, ADVERSARIAL_REVIEW, IMPLEMENTATION_PLAN and ACCEPTANCE_GATES in this package.

The reviewed upload reports commit `4cceb89` and contains a v0.15 vendored Cortex package. The working tree may contain newer v0.20 or MiCode integration work. Inventory it, preserve dirty/untracked files, and create a reviewed delta map. Do not reset/clean/stash destructively, unzip over the repository, replace current build packs, or allocate a conflicting CX/R identifier. Run `tools/check_source_binding.py --source <checkout>` from this package to identify reviewed-source drift; drift requires mapping, not overwriting.

## Architecture constraints

Reuse `axon-os` as authority owner, `Runtime` as the compatible program seam, `AxonHost` for interpreter host effects and Cortex's `Authorized` action pattern. Introduce a narrow `CheckExecutor` and typed compute envelope for registered checks; do not smuggle arbitrary native commands through `.axjob`. Separate engine, enclosure, guest kind and placement. `axon-vm` names the existing Firecracker launcher; `axon-wasm` is a browser interpreter build; the custom guest kernel is not yet a general workload executor.

Preserve interpreter-first behavior, existing schemas/hashes, verdicts, approval and confinement tests. Do not enable `codegen` + `serde-json` together. Driver dependencies are optional; do not introduce LLVM/vendor SDKs into minimal interpreter/browser builds or create cyclic crate dependencies.

## Execute bounded slices

Begin with T00 and T01. Produce baseline and design-delta artifacts, then implement the smallest compiling contract/authority slice. Continue through M1 before considering remote providers, RAM forks or learned routing. M1 must execute a real registered Cortex check on an explicitly verified isolated Linux profile with immutable inputs, aggregate reservation, cancellation, independent result evidence and no operator-workspace mutation.

Harden temporary resource naming, lifecycle cleanup and the test result channel rather than hiding these gaps behind an interface. Resolve the Linux guest's policy-delivery protocol and verify actual workload execution; never disable policy or count the custom kernel's demonstration halt as task completion. Translate every scoped grant without broadening it, or refuse.

After M1, implement M2 logical branches and CAS promotion. Memory checkpoints/forks, a bounded WASM host, scoped egress and one remote provider are separate optional packages with their own conformance gates. Do not build all eleven vendor drivers. Do not copy budget, principal or credential authority when cloning state. Never use historical replay to retry an uncertain external effect.

## Evidence and completion

Before each slice, identify exact source symbols, acceptance gates and regression tests. After each slice, report files changed, actual commands executed, results, unknowns and next bounded dependency. Register a product gate only after its nonvacuous test exists and is invoked by CI/host validation. Missing tools, KVM, browser or provider access are NOT_RUN/BLOCKED, not a pass. Fixture-model tests prove the fixture contract only.

Run the existing package-integrity gate separately. Do not modify vendored package hashes/manifests or claim that new source work changes the vendored package's NOT_RUN statuses. Store implementation execution evidence in the repository's actual registry/invokers following their current schema, after inspecting it.

At completion of M1, provide the precise diff, test receipts, profile/configuration evidence, migration/rollback notes and remaining unimplemented capabilities. Keep feature-off behavior compatible for existing trusted workflows, but refuse protected requests when no conforming backend is enabled. Never downgrade their isolation as a convenience fallback.
