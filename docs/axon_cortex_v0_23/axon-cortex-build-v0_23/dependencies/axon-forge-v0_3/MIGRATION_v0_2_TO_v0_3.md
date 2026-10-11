# Forge v0.2 -> v0.3 migration

The v0.2 files under `compat/v0_2-superseded-schemas/` are historical, not normative in v0.3.

| v0.2 generic schema | v0.3 replacement |
|---|---|
| `experiment.schema.json` | `forge-experiment-profile.schema.json` + canonical Axon experiment |
| `evidence.schema.json` | `forge-evidence-payload.schema.json` + CX-32 evidence identity |
| `provenance.schema.json` | `provenance-projection.schema.json` + UEG provenance |
| `resource-graph.schema.json` | `resource-projection.schema.json` + Compute Fabric/CX-34 source |
| `strategy.schema.json` | canonical Axon `StrategyArtifact` + `forge-strategy-profile.schema.json` |
| `evaluation.schema.json` | canonical evaluator identity + `evaluation-profile.schema.json` |

No v0.2 file may be interpreted as a new authority after migration.
