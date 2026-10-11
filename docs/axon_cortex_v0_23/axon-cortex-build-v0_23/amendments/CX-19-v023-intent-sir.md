# CX-19 v0.23 Forge integration amendment — IntentIR/SIR separation

IntentIR expresses desired outcome, constraints and policy-level meaning. Forge SIR expresses the semantics of a software program. Intent constraints may inform SIR verification or code generation, but the two representations are not aliases and neither silently overwrites the other.

## v0.23 acceptance gates

**G19-r23-intent-sir-separation:** IntentIR/AIR and Forge SIR retain distinct schemas, identities and ownership; any lowering/constraint bridge is explicit and evidence-bearing.

