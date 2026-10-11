# CX-32 v0.23 Forge integration amendment — Forge evidence projection

Forge does not own an evidence graph. Compile/test/benchmark/meta/migration/build observations are encoded as Forge evidence payloads attached to CX-32 UEG evidence identities. Provenance projections retain exact source/SIR/compiler/pipeline/environment/resource identities when material.

## v0.23 acceptance gates

**G32-r23-evidence-projection:** Every Forge observation admitted to learning attaches to a canonical UEG evidence identity; Forge-local records cannot substitute for required evidence lineage.

**G32-r23-provenance-projection:** Forge provenance records stable artifact/input/tool/environment references and remains a projection of CX-32 rather than a parallel lineage database.

