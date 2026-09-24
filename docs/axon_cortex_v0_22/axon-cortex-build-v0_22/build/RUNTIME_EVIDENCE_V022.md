# Runtime evidence and qualification — 0.22

## Three independent claims

`engineering_qualified` requires the required source/host/peer gates for the bounded release. `policy_activated` additionally requires current independent admission and a subsequent task reporting the adopted digest. `measured_improvement_supported` additionally requires the prespecified independent study evidence. They must never be inferred from counts of passing document tests.

The delivered [qualification ledger](../integration/V022_RUNTIME_QUALIFICATION.json) contains no live evidence. The [offline evidence checker](../tools/check_v022_runtime_evidence.py) only tests structural completeness of externally supplied evidence; it cannot authenticate a signature or prove that a test actually ran. Even a structurally complete synthetic ledger is `REQUIRES_EXTERNAL_VERIFICATION`, not qualified.

## Required source evidence per product gate

Record the gate ID, actual source revisions/build digests for both peers where applicable, exact test command and test case, resolved features/dependencies, invoker identity, host/profile/image/configuration, start/end time, nonzero assertion/case count, result, artifacts/logs and trusted evidence origin. Preserve NOT_RUN/BLOCKED when infrastructure is missing. A plan/reference test is not an executed Rust/interop test.

ACF-G00–G37 must be included for the operational closed loop, in addition to the new CX gates and the referenced legacy contract behavior on the selected use sites. Do not require the entire unrelated old backlog; do not waive the actual behavior requirements it owns. The source-owner mapping records how reused implementations supply current evidence.

## Candidate commands for the target checkout

These commands name existing crates/scripts in the provided snapshots. They have **not** been executed by this build. Inspect current AGENTS files, lockfiles and feature requirements before running. Add narrowly named new use-site tests after implementing them; a nonexistent test filter yielding zero tests is not a pass.

```bash
# Axon checkout, on the selected development host:
cargo build -p axon-core --no-default-features --bin axon
cargo test -p axon-os
cargo test -p axon-vm
cargo test -p axon-cortex
cargo test -p cortex-policy-adapter
cargo test -p axon-reflex
bash scripts/cortex_package_gate.sh

# MiCode checkout:
cargo test -p micode-git
cargo test -p micode-delegate
cargo test -p micode-verify
cargo test -p micode-persist
cargo test -p micode-core
```

Source-specific CLI/feature changes must be re-reviewed. The paired bridge test must exercise the actual MiCode session/headless/build-loop and Axon consumer, not two copies of a shared fixture parser. Physical Linux/KVM tests must test enclosure and cleanup on the exact host/profile. Presence of `cargo`, `/dev/kvm` or a launcher is only a prerequisite, never qualification.

## Nonvacuous fault matrix

Test denied/malformed submit with zero effects; context mismatch before first model token; missing or changed registered executable; revoked epoch at dispatch; crash before/after reservation/launch; duplicate operation with same/different input; lost acknowledgement with possible effects; cgroup/socket/process/disk cleanup; concurrent budget requests; receipt substitution; tampered candidate after check; empty mandatory-check match; cancelled losing branch with pending cost; profile withdrawal; competing policy CAS; rollback to revoked predecessor; stale return base; cross-tenant replay; and actual next-task effective policy.

A matrix row includes positive and negative assertions. An expected refusal must assert absence of effects, not just an error string. Actual process/storage restarts are required for durability claims. A test fixture can validate intended transitions but not physical persistence or confinement.

## Evidence intake

Reference/harness outputs are untrusted until independent authenticated intake verifies producer, command/source/profile pins and artifacts. Self-asserted `evidence_source=supervisor_observed` or a correct JSON hash is insufficient. The subject must not hold the verifier/admission signing authority. Cryptographic mechanism and truststore deployment must be chosen/pinned by the source owners, not invented by a model-facing schema.
