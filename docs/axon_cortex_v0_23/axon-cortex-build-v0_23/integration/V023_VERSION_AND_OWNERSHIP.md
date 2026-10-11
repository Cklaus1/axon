# v0.23 version and ownership

v0.23 composes two immutable artifacts: Cortex 0.22 and Forge 0.3. Forge is independently versioned because program-semantic/compiler contracts will evolve faster than Cortex cognition/governance.

Cortex 0.23 MUST reference Forge by version/hash. It MUST NOT copy Forge SIR/Transform/Meta schemas into Cortex-owned namespaces. The embedded Forge ZIP exists as a frozen integration input, not a second specification owner.
