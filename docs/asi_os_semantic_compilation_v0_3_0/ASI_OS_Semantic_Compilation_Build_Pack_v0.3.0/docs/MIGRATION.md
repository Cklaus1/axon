# Migration to v0.3

## v0.2 → v0.3 breaking wire changes

| v0.2 | v0.3 |
|---|---|
| `ir_version: asi.aiir/v0.2` | `plan_version: asi.semantic-plan/v0.3` |
| node `op` | node `mechanism` |
| capability `op` | capability `mechanism` |
| semantic meaning inferred from mechanism | required node `semantic_op` plus capability `semantic_ops[]` |
| local AI-IR described as primary execution IR | portable plan is a projection; Cortex profile maps to CX-04 AIR |
| no explicit Cortex owner profile | versioned Cortex v0.23 owner profile + dependency locks |
| strategy mostly a digest in experience rows | first-class StrategyArtifact schema |
| local experiment/evidence/resource envelopes | explicit projections to CX-33/CX-32/CX-34 owners |
| software semantics only through generic plan/coding examples | explicit Forge v0.3 binding and IntentIR/AIR/SIR separation |

There is no silent wire migration. Convert documents, rehash all pinned identities, revalidate, and regenerate approvals/evidence where semantics or authority changed.

## v0.1 → v0.2 history
The original v0.1 conversation draft was illustrative, not a deployed schema. v0.2 introduced bounded DAGs, protected evaluation, immutable repair, strict identity and offline conformance. Those safety decisions remain unless explicitly superseded above.
