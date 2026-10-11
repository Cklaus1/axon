# Semantic IR (SIR)

SIR is the durable representation of program meaning. Source syntax is a frontend concern.

## Required conceptual model

```text
Program
  Module[]
  Symbol[]
  Type[]
  Function[]
  Region[]
  Block[]
  Operation[]
  Value[]
  Effect[]
  CapabilityRequirement[]
  ResourceRequirement[]
  PlacementConstraint[]
  Contract[]
  Provenance[]
```

Every stable node MUST have an id and kind. Applicable nodes SHOULD carry type/effect/capability/resource information and provenance.

Implementations SHOULD support multiple levels: SIR-HIGH (application semantics), SIR-MID (control/data flow), SIR-LOW (memory/scheduling/target), then backend IR.

Every lowering SHOULD preserve traceability back to parent semantic nodes. Canonical serialization MUST be deterministic.
