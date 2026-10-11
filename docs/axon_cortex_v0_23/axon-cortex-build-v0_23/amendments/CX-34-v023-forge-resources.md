# CX-34 v0.23 Forge integration amendment — resource projection

Forge consumes a revision/epoch-bound semantic projection of CX-34/Compute Fabric resource truth for placement verification and optimization hypotheses. It does not create a second physical inventory, scheduler authority or reservation mechanism.

## v0.23 acceptance gates

**G34-r23-resource-projection:** Every Forge resource node/edge used for placement identifies authoritative CX-34/Fabric source and observation epoch; stale projections are revalidated before protected execution.

**G34-r23-no-second-registry:** Forge cannot register, reserve or authorize physical resources independently of the existing capability/Fabric owners.

