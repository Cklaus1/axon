# CX-13 v0.23 Forge integration amendment — Forge execution isolation

Generated compiler/meta/build/migration code executes through the existing isolated runtime seam. Forge contracts describe requested operations and limits; CX-13 owns confinement, cancellation, environment projection, process-tree control and effect receipts.

## v0.23 acceptance gates

**G13-r23-meta-isolation:** Meta/build/compiler-plugin execution is capability-scoped and isolated; undeclared filesystem/network/process/secret effects are denied and recorded.

**G13-r23-forge-receipt:** Every protected Forge execution used as evidence binds to an external execution receipt/environment identity rather than a self-asserted Forge success flag.

