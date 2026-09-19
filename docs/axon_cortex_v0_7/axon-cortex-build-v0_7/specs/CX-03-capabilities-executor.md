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
