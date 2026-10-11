# Release and Rollback Runbook

## Before enabling a capability
Confirm its gates, named owner, exact source revision, dependency lock and config/registry/evaluation digests. Verify current access policy and data-egress approval. Review unresolved tests and known limitations. Have a working kill switch that stops new dispatch and a known-good plan/strategy pointer.

## Rollout order
Offline fixtures -> real sandbox/read-only task -> internal evaluation/repair -> shadow optimization with effects disabled -> small owner-approved canary -> monitored expansion. Do not share credentials or production state with speculative candidates. Pin deployed artifacts and retain the evaluation evidence for each promotion.

## Incident triggers
Unexpected external effect, unexplained spend, stale/incorrect approval, cross-tenant access, acceptance on missing evidence, unexplained quality regression, duplicate action, or broken replay isolation triggers suspension of affected capability/strategy. Do not continue solely because a learned judge reports high confidence.

## Recovery
Stop new dispatch, preserve journal/effect/approval records, reconcile unknown actions and identify affected tenants/artifacts. Restore the known-good strategy/config pointer only after compatibility checks. Revoke stale receipts/caches. Completed business effects may require separately approved compensation; rolling back a plan does not undo them.

## Exact-revision release record
Include source HEAD, dirty-tree check, build and lockfile hashes, capability/provider versions, evaluation/experiment references, performed commands, exit codes, enabled scopes, approver and rollback target. Any change after the gate requires a new gate. Record unavailable provider version/billing evidence explicitly.
