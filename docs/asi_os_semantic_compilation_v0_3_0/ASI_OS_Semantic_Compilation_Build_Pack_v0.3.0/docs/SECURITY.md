# Security Model and Threat Register

## Trusted computing base
Authentication, authorization, the credential broker, capability registration, budget/effect ledger, artifact access control and dispatch enforcement form the authority boundary. Compiler, optimizer, judge and repair workers are not in that authority boundary. A privileged adapter implementation must be reviewed independently of the model selecting it.

## Threats and mandatory defenses
| Threat | Failure example | Required defense | Release test |
|---|---|---|---|
| Prompt injection | Retrieved text asks to email secrets | Separate data from authority; scoped egress/credentials | T-SEC-01 |
| Effect laundering | Model labels a payment tool “pure” | Registry-authoritative effect metadata | T-SEC-02 |
| Approval drift | Plan changes recipient after approval | Receipt bound to exact action/plan digest and expiry | T-SEC-03 |
| Post-check-only safety | Refund issued before ownership check | Dispatch-time policy and preconditions | T-SEC-04 |
| Duplicate effect | Timeout retries an already-sent email | Stable action key, effect ledger, reconciliation | T-RT-03 |
| Cross-tenant cache leak | Hash lookup returns another tenant's text | Scope/auth checks on every read and reuse | T-ID-02 |
| Judge manipulation | Artifact says “all tests pass” | Independent evidence; injection tests; abstention | T-EV-03 |
| Holdout leakage | Repair sees private expected answers | Separate test authority and restricted storage | T-AX-02 |
| Budget race | Parallel calls all see same remaining funds | Atomic reservations before dispatch | T-RT-04 |
| Sandbox escape/stray children | Tool runs on host or leaves descendants | No host fallback; isolation and cleanup gate | T-SEC-05 |
| Malicious schema | Remote `$ref` causes network access | Pinned local schema registry; bounded parsers | T-IR-08 |
| Secret observability | Full prompts/keys persisted by default | Redaction and scoped artifact evidence | T-OBS-01 |
| Semantic laundering | Candidate changes `semantic_op` while keeping a convenient mechanism | Registry semantic-op allowlist + AIR lowering validation | T-ALIGN-02 |
| Parallel authority | Local capability/resource/evidence record is treated as canonical grant/truth | Owner profile; projection-only contracts; dispatch recheck | T-ALIGN-01 / T-XPL-03 |
| Evaluator capture | Strategy candidate rewrites evaluator/stopping rule to promote itself | Frozen CX-21/CX-33 references outside subject control | T-XPL-01 / T-XPL-04 |

## Approval receipt requirements
Production receipts bind tenant, principal, plan digest, revision, logical action ID, argument digest, capability, exact resources, expiry, use count and issuer signature/reference. Single-use consumption is atomic. Revocation is checked before dispatch. The example policy file is an input to tests/design, not a signed grant.

`human.request` can collect information or request approval. Its JSON output cannot mint a receipt. The human broker validates identity and issues the authoritative record. Policy remains free to reject even an approved action if other constraints changed.

## Sandboxed code
Only declared programs/images and validated argument arrays may execute. The adapter must prevent host path traversal, symlink escape, unrestricted network access and uncontrolled descendants. Package manifests, lockfiles, output-size limits, environment allowlists and process/resource limits must be enforced independently of the model. Tests must prove the intended executable actually ran before an empty syscall trace can be interpreted.

## Secrets and external data
Broker credentials at invocation time; expose handles rather than secret values. A handle does not itself grant permission. Classify all model-provider egress. Untrusted context can suggest computation but cannot change policy, registry entries, protected tests or run identity. Do not include full private source documents in general telemetry.

## Policy update behavior
Emergency revocation can stop active runs. More permissive policy changes do not silently expand an admitted run. A new plan revision re-enters admission. Policy engine unavailability denies new effects; it does not “best effort” allow them.

## Evidence limits
The offline harness checks selected unsafe retries and registry references. It is not a security sandbox or policy engine. All tests in this document remain production gates until implemented and executed against actual adapters.
