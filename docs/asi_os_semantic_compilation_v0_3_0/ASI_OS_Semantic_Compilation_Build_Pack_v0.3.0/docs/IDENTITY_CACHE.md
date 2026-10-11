# Identity, Artifact Provenance and Cache Rules

## Identity hierarchy
Intent digest identifies approved desired behavior. Evaluation digest identifies the protected yardstick. Plan digest identifies executable structure and dependency/config references. Candidate ID identifies a proposal within a compilation batch. Run ID identifies one execution. Trial/arm identify an experimental observation. Logical action ID identifies an intended effect; attempt ID identifies one dispatch attempt. Never reuse a content hash as an authorization token.

## Stored provenance
Store exact source/artifact bytes or scoped references, parent revisions, compiler and repair templates, generator/evaluator versions, registry and configuration snapshots, capability implementations, data freshness and accepted evidence. Preserve generated code where needed for audit and exact rebuild. Unavailable provider versions remain explicitly unknown.

## Plan cache versus result cache
Plan cache stores reusable structure. Result cache stores outputs for exact authorized inputs with freshness and effect rules. A semantic embedding match can retrieve a candidate plan; it cannot establish that the new request has the same constraints. Admission performs schema, scope, dependency and acceptance compatibility checks before reuse.

Suggested plan-cache dimensions: tenant, task class, contract version, input/output schemas, capability/runtime compatibility, policy scope, environment and compilation profile. Suggested result-cache dimensions add exact input digests, implementation/model response version, decoding settings, rubric/context versions, data source freshness and allowed reuse class.

## Hard versus soft invalidation
Hard: revoked access, incompatible tool schema, expired approval, unsafe dependency, stale required data or changed protected criteria. Block use pending new admission. Soft: new model availability, lower price or a proposed faster plan. Benchmark a new candidate while retaining the active release. Do not silently recompile every workflow on every model announcement.

## Privacy and deletion
A hash can still reveal equality of sensitive low-entropy values. Scope indexes by tenant and use keyed identifiers when required. On deletion, invalidate references and derived caches according to the data policy. Logs retain minimal audit metadata without preserving unauthorized content. Cached artifacts require current access checks even when their original run was authorized.

## Reusable procedures
Promotion requires parameterization, boundary tests, cross-instance/task-slice evidence, tenant safety review, owner, version, deprecation policy and rollback. A learned preference for a procedure is not a package installation right. Public/global sharing is opt-in and stripped of private context. Model-dependent substeps remain explicitly nondeterministic.

## Cortex strategy, evidence and authority identity
A Semantic Compilation cache key is never a replacement for Cortex canonical identity. Under the v0.23 profile, strategy identity/lineage resolves through CX-29, causal observations attach to CX-33 experiments and CX-32 evidence, and resource snapshots carry the CX-34 epoch they describe. `owner_profile_id` and exact Cortex/Forge input locks are part of compilation provenance. Cache hits still require current CX-03 authority and current compatibility at dispatch; neither a prior approval receipt nor a prior resource projection is reusable as a grant.
