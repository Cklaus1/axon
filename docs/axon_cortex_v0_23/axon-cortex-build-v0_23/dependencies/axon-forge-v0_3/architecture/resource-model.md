# Resource Projection

Forge consumes resource observations from the authoritative Compute Fabric/CX-34 plane.

A Forge projection may describe compute/memory/storage/model endpoints, capacities, supported operations/dtypes, bandwidth/latency/cost, privacy/security domains, residency and current availability. Each projected node/edge MUST retain its authoritative source reference and observation epoch.

SIR can express placement requirements and the verifier can determine whether a proposed plan is semantically compatible with the observed projection. The projection itself grants no execution authority and does not prove that capacity remains available at dispatch; CX-03/CX-13/CX-34/Fabric recheck current state.
