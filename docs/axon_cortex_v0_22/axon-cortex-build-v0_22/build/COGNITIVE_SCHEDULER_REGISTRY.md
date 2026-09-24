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

## v0.18 ACE physical profiles

[P02/P03/P08](../schemas/ACE_EXECUTION_PROFILE.md) attach independently negotiated mechanics to existing capability entries and receipts. Hard-filter before ranking and recheck at dispatch. Capability class, physical mechanism and deployment mode are separate. Budget all attempts and authorized fallback; no new ACE plan or registry authority.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
