# Adversarial Acceptance Matrix

**Status convention:** `offline` means a matching package test exists; `required` means a production/native integration test must be built and executed. Offline checks are narrower than the full stated requirement.

| Test ID | Scenario and expected behavior | Evidence location / status |
|---|---|---|
| T-IR-01 | Unknown op/field or invalid version rejected | Invalid fixtures + unit tests / offline |
| T-IR-02 | Duplicate nodes, cycles and missing dependencies rejected | Linter tests / offline |
| T-IR-03 | Missing output port, unordered reference and type mismatch rejected | Linter tests / offline |
| T-IR-04 | Return cannot hide disconnected work | Linter tests / offline |
| T-IR-05 | Registry/intent/config/evaluation digest mismatch rejected | Bundle tests / offline |
| T-IR-06 | Unsafe write retry rejected | Registry + fixture tests / offline |
| T-IR-07 | Boolean is not an integer; JSON duplicates/NaN rejected | Parser/type tests / offline |
| T-IR-08 | Unknown remote schema reference does not trigger fetch | Offline registry test / offline; parser DoS / required |
| T-EV-01 | Missing/failed required evaluation blocks acceptance | Simulation/tests / offline |
| T-EV-02 | Bad document repaired then exact output reevaluated | Synthetic demo / offline |
| T-EV-03 | Prompt-injected candidate cannot rewrite judge criteria | Live judge/contract authority test / required |
| T-EV-04 | Probability, score and confidence are not conflated | Decision-envelope and arithmetic tests / offline; provider mapping / required |
| T-EV-05 | Calibration drift/invalid policy abstains | Real labeled calibration + drift test / required |
| T-EV-06 | Holdout protected from generation/repair | Data access audit / required |
| T-RP-01 | Repair invalidates changed node and transitive consumers only | Graph closure tests / offline |
| T-RP-02 | Stale parent revision cannot replace active plan | Durable supervisor CAS test / required |
| T-RP-03 | Repaired output cannot reuse stale passing evidence | Exact-target checks / offline subset; production evidence service / required |
| T-SEC-01 | Retrieved prompt injection cannot authorize secret egress | Policy/provider integration / required |
| T-SEC-02 | Compiler cannot relabel effect metadata | Registry tests / offline subset; authority registration / required |
| T-SEC-03 | Changed target/amount or expired approval blocks dispatch | Human broker + effect adapter / required |
| T-SEC-04 | Preconditions execute before effect | End-to-end dispatch test / required |
| T-SEC-05 | Sandbox never falls back to host; descendants cleaned | Real sandbox/process tests / required |
| T-RT-01 | Independent tasks run concurrently without data races | Real scheduler test / required |
| T-RT-02 | Crash recovery does not erase unknown effects | Fault-injection journal test / required |
| T-RT-03 | Repeated attempts do not duplicate logical action | Real idempotency/reconciliation test / required |
| T-RT-04 | Concurrent reservations cannot overspend authorized cap | Ledger stress test / required |
| T-RT-05 | Cancellation stops new work and records residual effects | Real adapters / required |
| T-ID-01 | Canonical identity stable under object-key order | Hash tests / offline |
| T-ID-02 | Cross-tenant result/plan cache access blocked | Real artifact/cache service / required |
| T-ID-03 | Captured replay does not invoke a real provider | Simulator architecture / offline; runtime egress test / required |
| T-AX-01 | Same task/arm across trials cannot collide | Trial-key tests / offline |
| T-AX-02 | Synthetic results cannot authorize production promotion | Release service test / required |
| T-AX-03 | All failed/compiler/evaluator/repair costs included | Experience contract / offline; billed ledger comparison / required |
| T-AX-04 | Quality noninferiority and rollback are independently gated | Preregistered trial + canary / required |
| T-OBS-01 | Trace has evidence but no raw secrets by default | Real telemetry privacy test / required |
| T-CFG-01 | Unknown/unsafe config rejected; no duplicate config authority | Schema tests / offline; repository integration / required |
| T-REL-01 | Final artifact revision matches tested revision | Release runbook + hash report / offline package; repository exact-HEAD gate / required |
| T-ALIGN-01 | Cortex owner profile maps canonical concerns without creating duplicate authority | `tests/test_alignment.py` + source owner-map snapshot / offline |
| T-ALIGN-02 | Semantic operation and execution mechanism are distinct; laundering is rejected | Schema/linter + alignment tests / offline |
| T-ALIGN-03 | IntentIR, AIR and Forge SIR remain distinct with explicit lowering/provenance | Integration contract tests / offline subset; live adapters / required |
| T-FORGE-01 | Forge version/hash pinned; SIR/Transform/Migration/BuildProgram remain Forge-owned | Input lock + owner-profile tests / offline; runtime integration / required |
| T-FORGE-02 | Compiler/build success cannot bypass independent acceptance or external execution receipt | Live Forge/CX-13/CX-21 integration / required |
| T-FORGE-03 | Coding truth resolver surfaces conflict and prefers current executable/local evidence | Versioned repository/compiler fixtures / required |
| T-XPL-01 | Strategy has stable version/lineage and experiment binds frozen hypothesis/falsifier/evaluator/stopping rule | New schemas + alignment tests / offline subset; experiment service / required |
| T-XPL-02 | Local evidence attaches to canonical evidence identity with effective inputs/environment/resource epoch | Evidence projection schema / offline subset; CX-32 integration / required |
| T-XPL-03 | Stale resource projection cannot authorize or reserve protected execution | Resource projection schema / offline subset; CX-34/CX-03 dispatch test / required |
| T-XPL-04 | Hard gates run before Pareto/utility ranking; invalid candidate cannot win on cost/latency | Documentation/schema conformance / offline subset; optimizer integration / required |

Each target repository must link these IDs to actual native tests and command outputs. Do not copy this table and mark everything green based on the Python harness.
