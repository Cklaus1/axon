# v0.23 — Forge integration without ownership duplication

- Pins the validated v0.22 Cortex package as immutable parent input.
- Pins standalone Axon Forge v0.3 as immutable dependency.
- Defines CX owner amendments rather than copying Forge normative definitions into Cortex.
- Adds a canonical `StrategyArtifact` contract at the Cortex/Axon seam.
- Maps Forge meta execution to CX-03/CX-13 instead of a Forge permission system.
- Maps Forge evidence/provenance to CX-32 and experiments to CX-33.
- Maps Forge resource views to CX-34/Compute Fabric instead of a second hardware inventory.
- Makes Forge SIR explicitly distinct from CX-19 IntentIR/AIR.
- Makes CX-30 repository improvement consume Transform/Migration/SemanticDiff.
- Makes CX-18 native promotion consume successful Forge artifacts after independent admission.
- Adds structured diagnostics, bootstrap/truth resolution, BuildProgram and MiCode compile/repair seams.
- Adds 16 v0.23 work packages and 31 proposed gates. All remain NOT_RUN in this documentation package.
