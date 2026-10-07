# Compute Fabric integration addendum for Axon 0.22

**Proposed integration of ACF v0.1.0; no Fabric implementation is shipped.** All original files are preserved under [the reference pack](compute-fabric-v0_1-reference/README.md). The [crosswalk](V022_FABRIC_CROSSWALK.json) is authoritative for master-task ownership.

## Required slice

| Original ACF task | 0.22 master owner | Requirement retained |
|---|---|---|
| ACF-T00 | B255 | Rebase current source and preserve regression evidence. |
| ACF-T01 | B258 (with B256/B257) | Exact profiles, strict contracts, identity/authority convergence. |
| ACF-T02 | B259 | Truthful scoped legacy adapter; no native sandbox claim. |
| ACF-T03 | B260 | Durable journal, reservations, cancellation/reconciliation. |
| ACF-T04 | B261 | Immutable workspace store and safe materializer. |
| ACF-T05 | B262 | Extract actual Firecracker launcher with CLI parity. |
| ACF-T06 | B263 | Real physically qualified hardened Linux microVM. |
| ACF-T07 | B264 | Actual registered Cortex CheckExecutor. |
| ACF-T08 | B271 | Logical branches, carved budgets and fenced CAS. |

All ACF-G00–G37 remain separate product obligations. The wrapper gates supplement them at cross-system seams; passing a wrapper's document tests cannot satisfy them. ACF-T09–T13/G38–G54 are retained but deferred. In particular ACF-G54 closes broader Fabric scope; the bounded 0.22 scope uses B285 instead and does not claim full Fabric completion.

## Source reconciliation

The Fabric pack originally reviewed a source archive carrying a vendored Cortex v0.15. Axon 0.22 uses the uploaded current source and v0.21 contract pack. Compare the measured per-file inputs instead of assuming identical archive labels. Preserve original Fabric reports as historical evidence; do not claim they reviewed MiCode or the new 0.22 bridge.

`WorkspaceSnapshot` remains Cortex observation; `WorkspaceVersion` is durable content. `ExecutionContextReceipt` is preflight, not ACF execution evidence. Original request schemas do not carry every bridge identity (for example arm/branch links); the new sidecar references the unmodified ACF request and preserves those extra identities. It does not silently insert fields into closed `acf-compute-request/1`.

## Narrow trusted host boundary

Reuse `axon-os` grant/supervisor admission and current epoch checks. Fabric driver resources are not authority. No direct worker call to a launcher or optional backend may bypass this path. Keep the first guest offline and run provider calls on the trusted MiCode host. Registered check resolution includes actual executable and dependency closure; a model-produced display name or argv vector is not enough.

Physical gate receipts must name host, source/build, exact engine/image/configuration, independent observer and nonzero assertions. Availability of a Firecracker binary or KVM device does not automatically qualify confinement. Missing infrastructure produces BLOCKED/NOT_RUN for this slice.

## Promotion boundaries

Workspace publication and active-policy selection are different operations with different evidence. ACF's CAS protects content version changes. Axon admission plus a scoped policy CAS protects future behavior. Both require durable intent, current authority and exact verified/admitted bytes. Rollback may pause; it never weakens the requested enclosure. RAM fork/resume semantics are outside M2 logical branching.
