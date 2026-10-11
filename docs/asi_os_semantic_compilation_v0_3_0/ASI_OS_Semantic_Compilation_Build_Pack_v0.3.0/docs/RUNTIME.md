# Runtime Scheduling, Effects and Recovery

## Cortex execution binding
In the Cortex v0.23 profile the portable Semantic Plan is not another VM. Its `semantic_op` is lowered/projected to CX-04 AIR semantics; `mechanism` selects an admitted implementation/capability. Software-semantic work may bind through Forge SIR/Transform/Migration/BuildProgram, but execution authority remains external to Forge. Protected dispatch uses CX-03 current authority and CX-13 qualified isolation/effect handling, with CX-34 resource state rechecked at dispatch. A successful local node or successful Forge build is not proof that an external effect occurred; use the CX-13/provider receipt and CX-32 evidence identity.


## State model
A plan is an immutable artifact. A run is a mutable durable state machine over that artifact. Work completion is distinct from release. Required statuses and transitions are in SPEC RT-001/002; production transition writes use compare-and-swap/version checks to reject stale worker completions.

The scheduler dispatches only when dependencies succeeded, outputs validate, requested resources are reserved, and current policy allows the exact action. Independent read-only nodes may run concurrently. Explicit or registry-derived effect conflicts serialize operations or cause admission failure.

## Write-ahead dispatch
Persist an invocation intent and its logical action ID before external dispatch. Persist provider request identity and response/effect state afterward. A lease handles worker liveness; it is not proof that a missing result means the external action did not happen.

| Failure point | Safe recovery |
|---|---|
| Before intent persistence | Nothing dispatched; start normally |
| After intent, before confirmed dispatch | Reconcile adapter dispatch state before retry |
| Provider succeeded, response lost | Query by idempotency/action ID or mark UNKNOWN_EFFECT |
| Result persisted, acknowledgment lost | Return recorded result; do not redispatch |
| Cancellation after provider accepted action | Record inability to cancel and reconcile outcome |

No universal exactly-once promise is made. Pure work is repeatable under pinned inputs/implementation. Idempotent writes require a real adapter/provider guarantee. Non-idempotent writes use one automatic attempt and human/operator reconciliation after ambiguity.

## Budget ledger
The run owns a cost envelope spanning compile, plan selection, work, verification, repair and retry. Reserve a capped maximum before concurrent dispatch; reconcile actual usage and record unavailable billing explicitly. Unknown cost is not zero. Maintain monetary precision in integer micro-USD. Record transferred business value separately from inference/service charges.

Wall time includes waiting and repair; an optional active-compute metric is separate. Human approval timeouts have explicit expiry. Persist reservations and settlement IDs so a crash does not duplicate charges in local accounting.

## Replay modes
`inspect` reads events only. `captured` injects recorded model/tool outputs into deterministic replay and disables live effects. `reexecute` is a new run under current authorization and budgets. Captured replay verifies controller behavior, not that the outside world or current model would respond the same way.

## Local repair
Changed node IDs plus transitive consumers are invalidated. A surviving upstream capture is reusable only under matching inputs, registry/config, artifact permissions and freshness. Any final artifact change invalidates previous acceptance of that artifact. Do not blindly reuse an assertion about a previous patch after further edits.

## Required crash tests
Kill workers before/after dispatch intent, before/after external acknowledgment, before/after result commit and during approval. Restart the control plane with outstanding reservations and effect ambiguity. Test cancellation races, duplicated provider callbacks, stale leases, replay with egress blocked, and duplicate logical action detection.

## Simulator boundary
The included simulator uses no threads, network, processes, durable journal or credentials. It consumes captures and executes built-in pure checks. It demonstrates ordering and evidence flow only; no concurrency/durability/security claim follows from it.
