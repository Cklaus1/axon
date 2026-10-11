# Axon Compute Fabric — source-reviewed specification pack v0.1.0

**Status: Draft / Proposed. No fabric implementation is included.**

This package reviews the supplied Axon source and specifies an additive Compute Fabric around existing authority and runtime seams. It does not overwrite the source, rename existing Cortex types, claim a v0.20 package review, or mark any new product gate as passed.

## Read first

[ACF-01 specification](specs/ACF-01-AXON-COMPUTE-FABRIC.md) defines architecture, ownership, profiles, authority, lifecycle, budgets, storage, checkpoints, forks, Cortex integration and the optional provider boundary.

[Source review](review/SOURCE_REVIEW.md) explains what exists, what is missing, and which earlier assumptions the code changes. [Source evidence excerpts](review/SOURCE_EVIDENCE.md) provide 36 exact path/line ranges. [Adversarial review](review/ADVERSARIAL_REVIEW.md) records the second-pass challenges and the resolutions folded into the specification.

[Implementation plan](build/IMPLEMENTATION_PLAN.md) contains 14 dependency-ordered work packages. [Acceptance gates](build/ACCEPTANCE_GATES.md) define 55 product gates, all **NOT_RUN**. [Bootstrap prompt](build/BOOTSTRAP_PROMPT.md) is ready to hand to an implementation agent against the actual working checkout.

## Principal decisions

**Axon retains authority.** Reuse `axon-os` grants, approval and supervisor admission; `Runtime` for compatible program execution; `AxonHost` for interpreter effects; and Cortex's authorized action and evaluation boundaries. Native and remote backends supply resources, not identities or permission.

**Describe reality, not a tier diagram.** The supplied `axon-vm` is the Firecracker microVM launcher, not a separate full-VM service. `axon-wasm` is a browser interpreter build. The custom guest kernel leaves full interpreter/VFS execution unfinished. Profiles describe exact engine/enclosure/guest/OS/architecture/placement combinations with explicit evidence status.

**Start with one protected execution path.** Implement a real registered Cortex check in an isolated Linux profile, immutable workspaces, journaled lifecycle, reservations, cancellation and trustworthy results. Add logical branches and CAS promotion next. RAM forks, a bounded WASM host, egress, one remote provider and learned routing are independently gated later work.

**Preserve provenance and compatibility.** Link existing episode, observation and run-record digests through a new versioned envelope. Separate task/trial/attempt/operation IDs, durable workspace contents and runtime-specific checkpoints. Never convert a worker's exit status or hash into independent verification.

## Source binding and scope

The input is `axon-ai-context-20260924(1).zip`, with 1,213 file entries. Its README reports source commit `4cceb89` dated September 24, 2026. That label was not independently Git-verified. The source manifest contains the measured archive digest and per-file SHA-256 values.

This is a targeted static review, not an audit of every source line. The uploaded bundle includes the vendored Cortex v0.15 package; it does not supply the newer v0.20 build pack discussed previously. Rebase the proposal against the actual latest checkout before implementation. Do not change a checkout just to make its hashes match this source snapshot.

## Validation

Executed source checks: 37 Python files parsed; 25 TOML files parsed; 126 shell scripts passed `bash -n`; the existing Cortex documentation-package gate passed and verified 113/113 pinned files. It ran **zero product gates**. Rust compilation/tests and Firecracker execution were **not run**, because this environment lacks `cargo`, `rustc`, Firecracker and `/dev/kvm`.

The new package validator checks task/gate integrity, source-evidence bindings, three JSON schemas, 15 synthetic schema fixtures, seven strict-parser cases and one canonical-identity check. It does not execute Axon, a VM or a provider. Read the actual generated report for the result.

```bash
# Requires Python 3.11+ and the jsonschema Python package.
python tools/validate_package.py --report reports/package_validation.json

# Compare the reviewed files against the actual target checkout (read-only).
python tools/check_source_binding.py --source /path/to/axon
# Optional complete archive-file comparison; extra checkout files still need intake.
python tools/check_source_binding.py --source /path/to/axon --all
```

`check_source_binding.py` exits 2 when a rebase is needed. It never modifies the source. The validator writes only the explicitly requested report.

## Files

| Path | Meaning |
|---|---|
| `specs/ACF-01-AXON-COMPUTE-FABRIC.md` | Normative proposed design. |
| `review/SOURCE_REVIEW.md` | Findings, readiness and measured review scope. |
| `review/SOURCE_EVIDENCE.md` | Original source excerpts with line numbers. |
| `review/ADVERSARIAL_REVIEW.md` | Threat/failure challenges and incorporated fixes. |
| `build/IMPLEMENTATION_PLAN.md` | Work packages, dependencies, edit surfaces and regression procedure. |
| `build/ACCEPTANCE_GATES.md` | Nonvacuous product-test obligations; all NOT_RUN. |
| `build/BOOTSTRAP_PROMPT.md` | Preservation-first implementation-agent instructions. |
| `contracts/*.schema.json` | Minimal untrusted request, backend profile and registered-check receipt envelopes. |
| `contracts/examples/` | Synthetic positive/negative fixtures, including intentionally invalid JSON-schema examples. |
| `manifests/` | Task/gate inventory, evidence ranges, source hashes and fixture expectations. |
| `reports/` | Actual validation outputs and the existing documentation-gate log. |
| `tools/` | Read-only source binding and proposal validation tools. |

The schemas deliberately cover a minimal vertical-slice envelope rather than claiming a full stable SDK. Syntax validation does not authorize a request, authenticate an issuer, or verify a backend. Do not run all negative fixture JSON files as if they were valid configuration.
