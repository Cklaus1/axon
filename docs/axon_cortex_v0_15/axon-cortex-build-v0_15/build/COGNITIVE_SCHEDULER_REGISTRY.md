# Cognitive Scheduler and Shared Capability Registry build guide

CX-34 is the OS-style dispatch layer for heterogeneous cognitive capabilities. Build the registry before the learned scheduler: capability identity, semantic contract, applicability, authority/effect ceiling, evidence tier, runtime requirements, cost/latency profile, failure modes and fallback must be queryable without model inference.

Dispatch order:
1. enumerate authorized/type-compatible/privacy-compatible capabilities;
2. apply hard evidence/risk constraints;
3. score remaining candidates with a versioned utility policy;
4. execute through existing AIR/capability paths;
5. record realized outcome/cost/cache state in CX-32;
6. preserve abstention/fallback reasons.

Initial policy may be deterministic. Learned routing is a later optimization target and cannot self-publish new registry entries.
