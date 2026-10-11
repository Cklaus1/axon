# Transform API — Normative v0.3

```text
Transform.apply(input_ir, context) -> TransformResult
```

```text
TransformContext {
  axon_experiment_ref: string?
  parameters: object
  capability_request_ref: string?
  resource_projection_ref: string?
  environment_ref: string
  verifier_ref: string
}

TransformResult {
  status: "applied" | "not_applicable" | "rejected" | "failed"
  output_ir: SemanticIR?
  semantic_diff: SemanticDiff
  provenance_projection: ProvenanceProjection
  diagnostics: Diagnostic[]
  verification_requirements: VerificationRequirement[]
  observed_inputs: ObservedInput[]
  forge_evidence_payloads: ForgeEvidencePayload[]
}
```

`applied` MUST include `output_ir`. `rejected` MUST identify the failed precondition/invariant/authority seam. A transform MUST NOT expand external authority or mutate the canonical evaluator/admission policy.
