# Axon Integration — Normative Mapping

Forge plugs into existing Axon owners; it does not extend Axon by cloning their state machines.

- `ForgeExperimentProfile` -> CX-33 canonical experiment.
- `ForgeEvidencePayload` / `ProvenanceProjection` -> CX-32 UEG evidence.
- `ForgeStrategyProfile` -> canonical versioned `StrategyArtifact`.
- transform/migration/pipeline candidate -> CX-11/CX-18 admission/promotion.
- `CapabilityRequest` -> CX-03 current grant lookup/recheck.
- meta/build execution -> CX-13 isolated executor.
- `ResourceProjection` -> CX-34/Compute Fabric observation.

Axon remains compiler-agnostic: it only requires stable artifact identities, hypotheses, evidence, constraints/objectives and strategy identities.
