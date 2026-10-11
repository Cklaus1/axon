# Neural Programs — reviewed implementation guide

**Owners:** CX-23 lifecycle, CX-36 formats/contracts, CX-11 admission, CX-34 registry.  
**Status:** proposed implementation. Package tests are not production inference or safety evidence.

## First delivery: format and contract, no model required

Start at B152–B155. Read `build/OWNERSHIP_AND_COMPATIBILITY.md`, map existing Axon types to the reviewed source contract, and implement strict `.nps` parsing plus bounded `.np` inspection. Use the inert examples in `fixtures/neural_programs/`; they must never register as active learned capabilities. Keep plain `.ax` semantics and existing parser/host ownership unchanged.

Produce canonical source and manifest digests, an exact asset/dependency inventory, and a detached release envelope. Tests must distinguish transport hash, artifact identity, validated structure, authentic issuer, policy admission and semantic correctness. A valid artifact with an untrusted issuer must remain non-active.

## Second delivery: one existing local runtime seam

Reuse the CX-23 owner and prior B77–B80 work instead of creating a parallel compiler/registry. Implement separate compiler and runtime interfaces; inference workers do not inherit training/provider credentials. Describe backend support before dispatch. Prefer an isolated local sidecar for candidate evaluation; embedded use requires a reviewed trusted-loader profile. Unsupported targets, model-state operations and missing assets refuse explicitly.

The first runtime returns `InvocationResult<DiagnosticSummary>` with a proposed value, exact invocation receipt and separate format/source/semantic validations. Use deterministic parsing/projection wherever the original value exists. The learned component may select spans or rank relevance; it cannot invent a path, diagnostic code, effect grant or proof witness.

## First pilot and baseline

Use a bounded corpus of permitted build logs split by repository/task/mutation family. Include exact-parser and current-incumbent baselines. Define success before evaluation: required-diagnostic recall, invented-field/error rate, schema validity, abstention, downstream repair quality, cost and latency. Thresholds/sample/uncertainty requirements belong to the owner-approved EvaluationPolicy, not a hard-coded demo number.

Evidence-deletion examples test Unknown/NeedMoreObservation. Adversarial logs contain fake system messages, success banners next to real failures, confusing paths, Unicode boundaries, truncated lines and copied-but-irrelevant diagnostics. Human labels are versioned annotations, not automatically ground truth. A failed specialization should be rejected, not trained repeatedly against the locked test until it appears to pass.

## Inner build loop

For each work package: locate the actual owner → define one observable invariant → write a failing positive/negative fixture → implement the smallest change → test parser/lifecycle/failure paths → record exact commands/digests → compare with the existing owner → update task evidence. A mocked model proves plumbing, not learning quality.

## Outer integration loop

Run source → candidate package → isolated load/invoke → source-bound validation → episode/evidence graph → independent accept/reject. Exercise cancellation, expiry, corruption, adapter hot-swap, unavailable dependencies, tenant changes and rollback. Verify that no release can be forged by editing manifest status. Test one target first; conversions get new identities and their own tolerance/task evidence.

## Adversarial loop

Attempt archive path/duplicate/size attacks; digest self-reference; malicious binary/parser code; forged admission; changed validator bundle; stale alias/cross-tenant cache; base-model fallback; private-to-public remote compile; remote timeout/retry spend reset; output schema success masking factual failure; and signed-but-revoked rollback. The package's inert tests cover only a subset of format attacks; product G36 gates require real target-runtime tests.

## Meta loop and stopping rules

After each bounded experiment decide whether the bottleneck is representation, data, model, calibration, runtime or a missing exact implementation. The meta loop may propose another candidate, not change the exam or contract. Stop on budget, insufficient evidence, weak labels or lack of improvement; report rejection/inconclusive status as such. Only an independently admitted canary may affect future live calls. Default automatic promotion stays disabled.

## External import and deployment

ProgramAsWeights support is optional B157. Do not call a hosted compile/inference service, publish a spec, or download models as part of offline parser tests. Acquisition, publication, compilation and inference have separate explicit grants and data policies. No imported `.paw` file becomes `.np` by filename alone.

A final deployment receipt binds exact artifact, runtime/precision, schema/validator, reliability, project and revocation generations. An invalid previous-good release cannot be reactivated. Rollback changes future behavior, not already-disclosed data or historical effects.

## v0.18 ACE without format change

[ACE P09](../schemas/ACE_EXECUTION_PROFILE.md) connects the existing learned-function lifecycle to physical dispatch/receipts. Keep reviewed CX-36 r0.2, strict source/package identity, detached release and ProposedValue unchanged. Package executable bytes before evaluation. ProgramAsWeights remains optional; prompt-only recipes remain non-neural. AN4 reuses the source-bound BuildLog pilot; rejecting a real candidate is valid.

## v0.19 Neural Program decision lowering

Keep CX-36 r0.2 unchanged. Host/compiler lowering may turn learned predicates into `DECIDE<T>` nodes, combine eligible nodes with `FANOUT`, `JOIN` their typed results, consult a matching calibration artifact, then `VERIFY` or `ESCALATE` under external policy. This optimization never embeds a permission grant or completion certificate inside `.np`.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
