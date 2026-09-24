---
id: CX-03
title: "Dynamic capabilities and transactional execution"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-02"]
first_stage: M1
implementation_evidence: []
---

# CX-03 — Dynamic capabilities and transactional execution

## Intent and source basis

Turn selected semantic actions into bounded effects without allowing model output to create authority. S2 pp.18–33 motivates opaque observed targets and freshness checks; S1 pp.12, 16 and 48 limits the guarantees available from the existing interpreter/FFI stack. See [SOURCES](../SOURCES.md).

## Decisive fork

Choose a trusted server-side grant registry and immutable workspace transactions. Do not treat enum membership, a signed-looking string, or a path prefix as authorization.

## Capability contract

A grant stores grant ID, issuer, principal/session, action kind, snapshot/epoch, allowed target set or namespace, read/write scopes, payload constraints, preconditions, expiry, invocation and resource bounds, and revocation state. The model sees a reference and readable description. The reference is meaningful only after trusted registry lookup. Child grants can attenuate parent authority and carved budgets, never enlarge them.

The selected action is a tagged union: Inspect(target), Search(scope), ProposePatch(targets, artifact), RunCheck(check_id), Revert(local_checkpoint), RequestContext(scope), Escalate(reason), DoneClaim(evidence_refs), or Blocked(reason). Additional action types require a manifest and recovery semantics. DONE has no execution authority.

A registered check maps to trusted executor configuration: program/entrypoint, argument schema, working directory, sandbox profile, effect permissions, environment/secret projection, and budgets. Arguments are arrays/typed values, never a shell command string. A generated test/build script is untrusted executable content even when invoked through a registered check.

## Payload and freshness validation

Generated patches are data. Parse/apply them only to the approved workspace; reject out-of-scope paths, traversal, symlink escapes, secret destinations and forbidden manifest changes. Validate new files through an explicitly permitted namespace. An inspected target can be valid yet semantically wrong: task correctness remains the verifier's job.

Prepare on an immutable snapshot. Before commit, validate expected content hashes and authority under a workspace lock or equivalent atomic compare-and-swap. Include dirty/untracked inputs and dependency/environment fingerprints. Avoid a check-then-write gap. Concurrent actions need compatible read/write sets and an atomic commit order; v0 is single-writer.

## Durable action state machine

`Proposed → Prepared → Validated → Running → Observed → Verified` is the normal path. Terminal alternatives are Refused, Failed, Canceled, or OutcomeUnknown. “Observed” means effect outcome recorded; “Verified” means the action/task contract was checked, not that every goal is complete.

Persist action ID and preparation evidence before effects. The execution attempt has an idempotency key and the host adapter declares at-most-once, deduplicated retry, or reconciliation-required semantics. A crash between an external effect and receipt is OutcomeUnknown. Reconcile against the external system or require operator review; never blindly rerun a non-idempotent effect.

Local patch rollback restores an immutable checkpoint and invalidates descendant artifacts/observations. It does not undo external calls or previously disclosed secrets. A test may generate local artifacts; include them in observed deltas and delete only inside its owned ephemeral workspace.

## Threat model and host dependency

Cover cross-session handle replay, principal impersonation, grant expiry, budget reuse, TOCTOU, symlinks/hardlinks, executable/tool replacement, compiler plugin/build-script execution and a worker attempting to mutate verifier assets. Where network is allowed later, destinations and redirects are executor policy, not model-provided permission. Strong host confinement is CX-13; unsafe host profiles remain disabled for real execution.

## Acceptance gates

**G03-forgery:** unknown, expired, wrong-principal and previous-snapshot grants all refuse without a side effect.

**G03-payload:** a valid Edit grant paired with a traversal, symlink or policy-file patch refuses; a permitted ordinary patch succeeds.

**G03-race:** mutate input between prepare and commit; exactly one compatible local transaction can commit and the stale one must reobserve.

**G03-crash:** inject crashes at every durable boundary. Recovery never blindly duplicates an external effect and identifies an unknown outcome.

**G03-budget:** nested child actions and speculative requests cannot exceed the parent's reserved aggregate budget; concurrent debits are atomic.

**G03-done:** a DONE claim with failing hidden checks cannot close the task, regardless of confidence or agent explanation.

## Build slices and exclusions

Build denial cases first, local snapshot patching second, isolated checks third. No generic shell tool, automatic production deployment, irreversible external effect, multi-agent concurrent writer, or capability self-minting in v0.

## v0.16 review amendment — Fallback is a new checked dispatch

Each fallback/retry receives a child invocation identity and inherits the parent budget/deadline. Revalidate authority, payload, target state and revocation immediately before dispatch. Never treat denial as a reason to retry through a broader-privilege provider. A worktree provides version separation, not confinement; build scripts remain untrusted under CX-13.

**G03-fallback-recheck:** a revoked grant, exhausted parent budget or denied destination cannot be revived by cache hit, retry, alternate provider or a new child action ID.

See `build/REVIEW_INVARIANTS.md` for the shared interpretation and `review/BUILD_ADVERSARIAL_REVIEW.md` for attack cases.

## v0.18 ACE integration

ACE decisions/proposals never grant effects. Current grants apply at inference dispatch and again at downstream action commit; physical attempt receipts do not prove tool execution. A learned or remote advisory result cannot bypass refusal or protected completion. See [ACE execution profile](../schemas/ACE_EXECUTION_PROFILE.md).

## v0.20 amendment — schema decisions and lightweight Reflex specialization

Use the [schema-decision profile](../schemas/SCHEMA_DECISION_PROFILE.md) at the existing capability compiler/executor seam. Tool descriptors and safety annotations remain untrusted. The local registry independently resolves actual effects, current tool identity, grants and confirmation requirements. Revalidate schema/registry/snapshot at prepare and dispatch; a same-named replacement is not the same capability. Typed arguments are proposals, not preapproved commands. Unsupported schemas cannot silently become a permissive executable approximation.

**G03-schema-trust:** Injected descriptions, readOnlyHint/destructive hints, same-named schema replacement and high-confidence actions cannot grant effects or bypass the trusted local resolver, current grant, freshness and independent verification.

## v0.21 research integration amendment

This additive amendment is normative for the explicitly selected v0.21 profile. Existing authority and wire-format owners remain unchanged. The [research integration contract](../build/RESEARCH_INTEGRATION_V021.md) and [requirements matrix](../integration/RESEARCH_REQUIREMENTS.json) specify the complete profile and source-backed migration. Older versioned profiles remain separately identifiable; none of these declarations establishes runtime implementation.

### Acceptance gates added in v0.21

**G03-r21-speculation-no-effects:** Speculative branches produce isolated proposals only; no side-effectful tool or commit occurs before the authoritative grant, verification barrier and chosen-branch decision.

**G03-r21-speculation-terminal:** Cancelled/rejected/timed-out branches cannot commit or reenter acceptance; unknown effects require reconciliation before retry, and duplicate acceptance cannot commit twice.

## v0.22 amendment — closed-loop Axon / Compute Fabric / MiCode integration

The [0.22 integration contract](../build/CLOSED_LOOP_INTEGRATION_V022.md) and [source-bound work packages](../build/WORK_PACKAGES_V022.md) extend this existing owner. They do not introduce a parallel authority, learning store, sandbox claim or schema reinterpretation. Original 0.21 research profiles remain available, but the bounded 0.22 pilot uses only its explicitly qualified dependency slice. The [Fabric crosswalk](../integration/V022_FABRIC_CROSSWALK.json) preserves ACF-01 M0-M2 obligations; the [MiCode consumer delta](../integration/MICODE_V022_CONSUMER_DELTA.md) binds actual peer work. All added runtime obligations are unexecuted proposals in this pack.

**G03-r22-authority-intersection:** Every new submit, inspect, cancel, reconcile, promotion and peer-import route enforces current local authority and resource scope; possession of Axon policy, Fabric handle or MiCode receipt grants no additional capability.

**G03-r22-workspace-import:** Materialization rejects traversal, absolute paths, unsafe links/devices, namespace collisions and quota expansion; immutable inputs and outputs have retrievable byte-complete manifests with explicit omissions.

**G03-r22-trial-isolation:** Incumbent/challenger workspaces and mutable build caches cannot contaminate one another or the integration checkout; an unauthorized path write is blocked at the actual enforcement boundary, not merely detected in a later diff.

**G03-r22-physical-isolation:** A real Linux microVM blocks host source/credential access and egress, enforces mount/child-process/resource/output limits, and survives adversarial guest behavior with independently observed host checks.

**G03-r22-check-effects:** The actual check dispatch is within current approved authority and profile requirements; arbitrary executable/argv substitution and stale authority are refused with zero backend effects.

**G03-r22-dispatch-recheck:** Permissions, scope, revocation epoch and candidate identity are rechecked at actual tool execution after shortlisting and immediately before effects, including nested delegation and retries.

**G03-r22-joint-bypass:** All public/child/fallback/retry routes reject attempts to bypass authority, host/guest confinement, verifier separation or forbidden write sets, with nonzero actual source/host tests on the selected profile.
