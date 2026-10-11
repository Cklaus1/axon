# CX-15 v0.23 Forge integration amendment — semantic compiler substrate

Forge SIR is the generic program-semantic representation below source frontends and above target-specific backends. CX-15 consumes Forge SIR/Transform/Backend contracts for Axon language/compiler work while retaining its existing proof/parity/native-promotion obligations. Forge does not replace the Axon compiler; it provides a generic substrate and conformance contract.

## v0.23 acceptance gates

**G15-r23-sir-boundary:** Source/frontend lowering into Forge SIR has deterministic identity, explicit contracts/effects/resources and traceability; SIR is not confused with IntentIR/AIR.

**G15-r23-transform-abi:** Compiler/program transformations use the Forge Transform/TransformResult ABI with declared preconditions, preserved invariants, semantic diff and verification requirements.

**G15-r23-build-backend:** BuildProgram/backend adapters declare inputs, toolchain/target identity and outputs; backend success cannot bypass semantic verification or external authority.

