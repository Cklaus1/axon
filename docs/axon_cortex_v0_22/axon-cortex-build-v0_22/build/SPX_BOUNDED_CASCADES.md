# SPX — bounded speculative response proposals

**Owners:** CX-26 composition, CX-03 transactional effects, CX-04 interpreter, CX-13 resources. **Tasks:** B241/B242/B244; optional B254. **Reference:** RES-RLM.

RLM-Cascade motivates cheap drafts with conditional verification or bypass. Axon initially narrows this to isolated response/patch proposals. This is not token-level speculative decoding, does not promise identical target-model distributions, and does not imply universal savings.

## Initial state machine

```text
CREATED -> RESERVED -> DRAFTING -> DRAFT_READY -> VERIFYING
VERIFYING -> VERIFIED -> SELECTED -> AUTHORIZE_EFFECT -> COMMITTED
VERIFYING -> REPAIRING -> DRAFT_READY
any noncommitted phase -> REJECTED / CANCELLED / TIMED_OUT / FAILED
unknown external effect -> RECONCILE (never optimistic retry)
```

Direct-incumbent execution is always an available qualified strategy. A skip path needs separately admitted domain-specific correctness policy and cannot bypass required deterministic final checks. CLM selecting the only candidate with probability one does not qualify that path. Schema-critical tool selection may bypass speculation entirely.

All candidates and repairs have immutable bytes/digests, task/attempt/branch IDs, generator/model revision and verification-policy identity. Verification binds the exact candidate digest and relevant environment state. Any repair, formatting rewrite or context-dependent patch modification invalidates previous verification and must be checked again. “Verifier improved the draft” is not the same as “the improved draft was verified.”

## Effect barrier

Speculative branches cannot dispatch effectful tools, write the live workspace, mint grants or commit a patch. They may produce isolated artifacts under existing sandbox/grant policy. Only the selected, independently checked bytes reach the existing authoritative authorization/transactional executor. Draft streaming is labeled uncommitted; it cannot be interpreted as a committed tool call.

Exactly one chosen branch may cross the barrier, at most once. Cancellation and rejection are terminal for that branch. Concurrent branch completion must not race around selection or authorization. Unknown effects require host reconciliation before retry; a transport timeout is not proof nothing happened. A deterministic permission denial never triggers a retry with a weaker scope.

## Resource policy

Reserve the worst permitted total cost across draft, verifier, repair, fallback, selection, permutations and retries before launching work. Declare maximum rounds, branches, selector depth, deadline and fallback choices. Unknown usage retains a bounded reservation. Cancellation does not erase provider charges, and cancelled calls must be reconciled without double charging. Insufficient remaining budget refuses the speculative path or takes a previously affordable direct baseline.

Do not launch N candidates because they are individually cheap without accounting for aggregate resource contention. Best-of-N is optional B254; compare all-candidate generation plus selection and verification against an equivalent-budget direct control. Oracle best-of-N is an upper bound on available candidates, not the actual selector's success rate.

## Qualification

Start with deterministic draft and verifier fixtures, including repaired-digest mismatch, cancellation-before-commit, permission denial, concurrent completion, unknown-effect and price/usage failure. Then test one authorized real model path through the existing `PatchGenerator`/runner rather than a parallel bypass. Record final independent task checks and full costs.

The executable package state machine demonstrates bounded reference invariants only. It does not execute tools, authenticate principals, create a sandbox or implement a production concurrency protocol. Actual source tests must prove those properties separately.
