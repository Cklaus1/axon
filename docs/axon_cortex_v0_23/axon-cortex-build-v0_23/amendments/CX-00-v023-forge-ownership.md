# CX-00 v0.23 Forge integration amendment — ownership and compatibility

Forge v0.3 is a pinned peer/substrate, not a new Cortex authority. The 0.22 parent package remains immutable. v0.23 composes the parent with Forge through explicit owner profiles and typed references. A Forge-local cache or projection cannot supersede canonical Axon/Cortex/Fabric owners.

## v0.23 acceptance gates

**G00-r23-forge-owner-map:** Every Forge concern maps to one canonical existing authority or an explicitly Forge-owned program-semantic contract; no duplicate global experiment, evidence, capability, admission or resource authority is introduced.

**G00-r23-parent-preservation:** The exact v0.22 parent archive and Forge v0.3 dependency are hash-pinned; the integration delta cannot silently rewrite either input.

