# v0.3 — ownership-normalized Forge

- Keeps v0.2 Semantic IR, meta-execution, transform, migration, build-program, diagnostics, bootstrap/truth-resolution and conformance concepts.
- Replaces generic Forge experiment/evidence/provenance schemas with **Axon-bound profiles/payloads**.
- Replaces the authoritative Forge resource graph with a **resource projection** over Compute Fabric/CX-34 facts.
- Adds explicit capability-request schema: meta/build/transform code requests authority but never mints it.
- Makes `StrategyArtifact` an external Axon/Cortex contract; Forge supplies a compiler/build/repair/retrieval strategy profile only.
- Adds semantic-diff and build-program normative schemas.
- Adds Cortex v0.23 crosswalk and non-duplication conformance vectors.
