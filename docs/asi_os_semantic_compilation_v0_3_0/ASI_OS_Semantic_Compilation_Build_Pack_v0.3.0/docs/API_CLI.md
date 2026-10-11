# Proposed APIs and CLI

**Status:** design contract. Only `tools/check_pack.py` and `python -m harness.simulate` are executable commands supplied here. No `asi` binary or HTTP server is implemented.

## Service operations
| Operation | Request | Response | Authority |
|---|---|---|---|
| `CompileIntent` | intent_ref, context_refs, profile_ref, request_key | status, candidate_refs, diagnostics_ref, provenance_ref | Compile scope only |
| `ValidatePlan` | plan_ref, registry_ref, evaluation_ref | findings, validated_document_digests | No execution grant |
| `StartRun` | plan_ref, inputs_ref, admission_request_key | run_id, admitted_bindings_ref, state | Authenticated current principal |
| `GetRun` / `StreamEvents` | run_id, cursor | state / monotonic events | Tenant-scoped read |
| `SubmitHumanDecision` | pending_request_id, exact_scope_digest, decision | broker-issued receipt or denial | Authenticated reviewer |
| `EvaluateArtifact` | contract_ref, target_refs, evidence_refs | evaluation_result_refs | Evaluation scope |
| `ProposeRepair` | repair_request_ref | candidate_revision_ref | No direct write to active run |
| `CancelRun` | run_id, expected_state_version, reason | status + unresolved_effects | Owner/operator |
| `ReplayRun` | run_id, mode=inspect/captured | separate replay_id, evidence | Egress disabled |
| `SubmitExperiment` | preregistration_ref, eligible_arms | experiment_id or rejection | Experiment grant |
| `PromoteStrategy` | report_ref, approval_ref, expected_active_digest | new active pointer or conflict | Release authority |

Request keys deduplicate transport retries only within tenant and operation scope. They do not replace logical external action IDs. All write operations return conflict on stale expected state/digest. Input artifact access is checked at request and execution time.

## Error taxonomy
`INVALID_SCHEMA`, `INVALID_GRAPH`, `UNKNOWN_CAPABILITY`, `TYPE_MISMATCH`, `NEEDS_CLARIFICATION`, `UNSUPPORTED_FEATURE`, `POLICY_DENIED`, `APPROVAL_REQUIRED`, `BUDGET_EXCEEDED`, `DEPENDENCY_CHANGED`, `EVALUATION_FAILED`, `EVALUATOR_UNAVAILABLE`, `UNKNOWN_EFFECT`, `STALE_REVISION`, `CANCELLED`, `INTERNAL_ERROR`.

Errors carry a stable code, safe human explanation, retryability, evidence reference and no secrets. A retryable transport error does not automatically mean the business operation is safe to repeat.

## Proposed CLI experience
```text
asi compile intent.json --profile balanced
asi inspect plan.json
asi verify plan.json
asi run plan.json --inputs inputs.json
asi evaluate RUN_ID
asi explain RUN_ID
asi trace RUN_ID
asi replay RUN_ID --mode captured
asi bench experiment.json
asi promote REPORT_REF --approval APPROVAL_REF
```

`verify` explains which checks ran and which remain runtime gates. `explain` uses trace/plan evidence. A command must never print a confidence number as a blanket guarantee of correctness. `replay` refuses a request to perform live effects; a new `run` is required for reexecution.

## Wire versioning
Client and server negotiate contract family and supported operations explicitly. Unknown versions/fields fail closed. Local schemas define artifact envelopes; they are not a generated OpenAPI implementation. Add OpenAPI or native RPC definitions after repository transport discovery, rather than inventing a competing service API now.
