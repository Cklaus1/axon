# Forge v0.3 Ownership Matrix

| Concern | Authority | Forge role |
|---|---|---|
| Program semantics / SIR | Forge | owner |
| Transform/meta/migration ABI | Forge | owner |
| Diagnostics/build/backend contracts | Forge | owner |
| Experiment identity/lineage | Axon CX-33 | profile/attach |
| Evidence/provenance graph | CX-32 UEG | payload/attach |
| Admission/promotion | CX-11 / CX-18 | candidate only |
| Capability grants | CX-03 | request only |
| Isolation/runtime effects | CX-13 | request/receipt only |
| Physical resources | Compute Fabric / CX-34 | projection only |
| Intent semantics | CX-19 IntentIR | distinct input constraint |
| Cognitive/self-improvement policy | Cortex CX-29 | consumer/proposer |
| Coding workflow | MiCode CX-16 | consumer |

## Non-duplication invariant

A conforming Forge MUST NOT silently create a second global owner for any delegated concern. A local cache/index is allowed only as a revision-bound projection of the authoritative owner.
