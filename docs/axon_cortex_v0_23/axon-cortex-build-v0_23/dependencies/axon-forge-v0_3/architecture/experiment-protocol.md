# Forge -> Axon Experiment Protocol

Forge does not create a second experiment lifecycle.

A Forge candidate produces a `ForgeExperimentProfile` containing domain-specific references such as SIR, transform, pipeline, compiler, backend, semantic diff and build artifact. The profile MUST attach to a canonical Axon experiment identity owned by CX-33.

The Axon experiment owns parent lineage, hypothesis/falsifier, mutation classification, environment identity, strategy identity, evaluator identity, metric vector, conclusion/confidence, retention and promotion state.

Forge MAY request an experiment; it MUST NOT self-promote the result.
