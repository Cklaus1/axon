# Observability and Operator UX

## Required views
The task view shows the approved goal, assumptions, constraints and input scope. The plan view shows dependencies, intended resource class, estimates, requested permissions and evaluation coverage. The execution view shows exact bindings, node states, actual costs, waits, retries and evidence. The repair view shows parent/new revision and invalidated descendants. The experiment view shows arms, trial counts, quality/coverage/cost and promotion state.

## Approval screen
Show the exact action, principal/tenant, target resource, data leaving the system, amount where relevant, current plan revision and expiry. The approval is invalidated by a changed recipient, amount, target or action arguments. Do not bundle unrelated high-impact actions into an opaque “approve plan” button.

## Evaluation screen
Separate deterministic pass/fail, learned probability, rubric score, provider confidence, calibration applicability and human preference. Show missing evidence and abstention clearly. A result marked “accepted” links to the contract and exact evaluated artifact. A failed check remains visible after a repair, linked to the superseding evidence rather than erased.

## Event fields
Events include event ID, timestamp, tenant/run/task/trial/arm identity, plan/config references, node/attempt identity, type, safe metadata and evidence references. Use monotonic sequence numbers within a run for replay and idempotent ingestion. A duplicate event does not double-count cost. Source-system clock time and elapsed monotonic duration are distinct.

Model accounting separates uncached input, cached input and output tokens where reported; missing billing data remains missing. Report compile/execute/evaluate/repair costs separately and summed. Policy and approval decisions have receipt references, not full credentials.

## Human-centered quality
For coding, show the exact tested patch and commands. For research, show which claims have support and whether evidence is stale or contradictory. For UI, keep usability task completion, accessibility checks and human preference separate; capture viewport/context for visual comparisons. A vision judge's style preference is not the user's preference.

## Explanation contract
Explain “this node was selected by policy v3 because its eligible set and logged objective favored lower predicted latency,” backed by actual records. Do not claim access to a model's internal reasoning. Distinguish predicted, observed and inferred explanations in the UI.
