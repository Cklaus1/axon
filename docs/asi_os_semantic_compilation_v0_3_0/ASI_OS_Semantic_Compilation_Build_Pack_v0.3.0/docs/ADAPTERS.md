# Adapter Contracts

## Shared requirements
Every adapter declares a capability identity/version, detailed argument/result schemas, authoritative effect classification, replay class, idempotency contract, scope requirements, resource/cost bounds, cancellation behavior and error taxonomy. Validate requests before crossing the boundary and responses before they enter the graph. Provider metadata and credentials are brokered; no ambient global secrets.

## Compiler / repair adapter
Input: normalized goal, scoped context, immutable acceptance constraints and remaining budget. Output: candidate reference, diagnostics, provenance and usage. A repair proposal also includes expected parent digest and permitted edit surface. No direct production writes. Return unsupported when the core IR cannot express the task.

## Model adapter
Resolve abstract resource requests to an approved concrete model route snapshot. Record requested and returned model identities, provider request ID, token counts by billing category, service cost or unknown status, decoding configuration and output schema validation. Providers may not guarantee reproducible outputs; captured replay is separate. Provider errors and transport ambiguity are distinct.

## Jev-like decision adapter
Accept state plus atomic typed questions, candidate IDs/rubric and context/evidence references. Normalize Choice, Score and Noul without discarding semantics [S03–S05]. Pin model/rubric/prompt versions for calibration and treat unknown resolution as a new unvalidated operating condition. Return distributions and provider confidence separately. A failed or malformed response is `error`, never a fabricated neutral score. No assumption that this adapter produces prose critique.

## Deterministic evaluator
Accept exact artifact and relevant independent evidence. Return verdict, checker implementation, observed predicates and target digest. Examples: frozen test suite, schema checker, exact fixture comparison. The implementation must not trust the artifact's own statement that tests passed. Validation data owned by the generator is not an independent holdout.

## Sandbox / code adapter
Accept immutable image/program identity and structured arguments, an isolated workspace and explicit egress/resource grants. Return exit status, artifact descriptors, bounded stdout/stderr references and usage. On cancellation, clean the entire owned execution scope; never use broad process-kill patterns. Treat copied test artifacts as untrusted and verify the intended binary/command ran.

## Tool / effect adapter
Take exact resolved arguments and an authorization receipt. Describe external idempotency and reconciliation. Distinguish no effect, confirmed effect and unknown effect. Retries use a stable logical action key but new attempt IDs. A write may not be considered replayable because its endpoint happens to accept repeated requests.

## Human broker
Present scope, exact action arguments, target identity, estimated cost and expiry. Collect authenticated approval/rejection and issue a bound receipt. Changed targets require new approval. Human labels for quality evaluation are separately recorded and do not automatically authorize business actions.

## Axon adapter
Receive eligible candidates and logged feature definitions. Return chosen candidate ID, policy version, allowed exploration information and uncertainty/provenance. Record observations with repeated-trial identity and externally checked outcomes. Record-only is the initial mode; an unavailable optimizer falls back to a preapproved baseline only.

## Minimum conformance suite
Timeout, malformed result, missing usage, wrong model version, unauthorized data egress, duplicate dispatch, cancellation, stale receipt, output-schema mismatch and unknown-effect reconciliation are tested for each applicable adapter. The pack contains contracts and synthetic registry entries, not working provider adapters.
